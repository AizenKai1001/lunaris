//! The sound card. [`Output::open`] starts a thread that keeps a few small buffers queued with whatever the [`Mixer`] renders next.
//! On Windows it uses the system's `waveOut` (no library to install). On Linux it plays through PulseAudio's "simple" interface, which PipeWire
//! also offers: the library (`libpulse-simple.so.0`) is looked up when the sound is opened, so nothing has to be installed to build, and a machine
//! without it just has no sound. On other systems opening reports that there is no output; the mixer can still be rendered to samples.

use crate::mixer::Mixer;
use std::sync::atomic::AtomicBool;
#[cfg(any(windows, target_os = "linux"))]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// A running sound output; dropping it stops the sound and closes the device.
pub struct Output {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Output {
    /// Open the default sound device and start pulling samples from `mixer`.
    pub fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Output, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = start(mixer, stop.clone())?;
        Ok(Output { stop, thread: Some(thread) })
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn start(_mixer: Arc<Mutex<Mixer>>, _stop: Arc<AtomicBool>) -> Result<std::thread::JoinHandle<()>, String> {
    Err("no sound output on this platform yet (the mixer still renders to samples, and `record_audio` writes a WAV file)".to_string())
}

#[cfg(target_os = "linux")]
fn start(mixer: Arc<Mutex<Mixer>>, stop: Arc<AtomicBool>) -> Result<std::thread::JoinHandle<()>, String> {
    use crate::mixer::RATE;
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::sync::mpsc;

    #[repr(C)]
    struct SampleSpec {
        format: c_int,
        rate: u32,
        channels: u8,
    }

    #[repr(C)]
    struct BufferAttr {
        maxlength: u32,
        tlength: u32,
        prebuf: u32,
        minreq: u32,
        fragsize: u32,
    }

    #[link(name = "dl")]
    extern "C" {
        fn dlopen(file: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    type New = unsafe extern "C" fn(*const c_char, *const c_char, c_int, *const c_char, *const c_char, *const SampleSpec, *const c_void, *const BufferAttr, *mut c_int) -> *mut c_void;
    type Write = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
    type Free = unsafe extern "C" fn(*mut c_void);
    type Drain = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;

    const RTLD_NOW: c_int = 2;
    const PA_STREAM_PLAYBACK: c_int = 1;
    const PA_SAMPLE_S16LE: c_int = 3;
    const FRAMES: usize = 512;

    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let thread = std::thread::Builder::new()
        .name("lunaris-audio".to_string())
        .spawn(move || {
            // SAFETY: the library is opened by name and its four functions are looked up by their documented names and called with their documented
            // argument types; the stream handle belongs to this thread alone and is freed before the thread ends; the sample buffer outlives every write.
            unsafe {
                let mut lib = std::ptr::null_mut();
                for name in ["libpulse-simple.so.0", "libpulse-simple.so"] {
                    let c = CString::new(name).expect("no NUL");
                    lib = dlopen(c.as_ptr(), RTLD_NOW);
                    if !lib.is_null() {
                        break;
                    }
                }
                if lib.is_null() {
                    let _ = tx.send(Err("PulseAudio's client library (libpulse-simple.so.0) is not installed, so there is no sound".to_string()));
                    return;
                }
                let sym = |name: &str| dlsym(lib, CString::new(name).expect("no NUL").as_ptr());
                let (pa_new, pa_write, pa_free, pa_drain) = (sym("pa_simple_new"), sym("pa_simple_write"), sym("pa_simple_free"), sym("pa_simple_drain"));
                if pa_new.is_null() || pa_write.is_null() || pa_free.is_null() || pa_drain.is_null() {
                    let _ = tx.send(Err("libpulse-simple.so.0 does not have the functions expected".to_string()));
                    return;
                }
                let (pa_new, pa_write, pa_free, pa_drain): (New, Write, Free, Drain) = (std::mem::transmute(pa_new), std::mem::transmute(pa_write), std::mem::transmute(pa_free), std::mem::transmute(pa_drain));
                let spec = SampleSpec { format: PA_SAMPLE_S16LE, rate: RATE, channels: 2 };
                // a short queue: about 25 ms on the server, a request every 6 ms
                let attr = BufferAttr { maxlength: u32::MAX, tlength: (RATE / 40 * 4), prebuf: u32::MAX, minreq: (RATE / 160 * 4), fragsize: u32::MAX };
                let (app, stream) = (CString::new("Lunaris").expect("no NUL"), CString::new("game").expect("no NUL"));
                let mut error: c_int = 0;
                let handle = pa_new(std::ptr::null(), app.as_ptr(), PA_STREAM_PLAYBACK, std::ptr::null(), stream.as_ptr(), &spec, std::ptr::null(), &attr, &mut error);
                if handle.is_null() {
                    let _ = tx.send(Err(format!("PulseAudio would not open a stream (error {error}): is a sound server running?")));
                    return;
                }
                let _ = tx.send(Ok(()));
                let mut scratch = vec![0.0f32; FRAMES * 2];
                let mut pcm = vec![0i16; FRAMES * 2];
                while !stop.load(Ordering::SeqCst) {
                    {
                        let mut m = mixer.lock().unwrap_or_else(|p| p.into_inner());
                        m.render(&mut scratch);
                    }
                    for (dst, src) in pcm.iter_mut().zip(scratch.iter()) {
                        *dst = (src.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                    // blocks until the server has room: that paces the loop
                    if pa_write(handle, pcm.as_ptr() as *const c_void, pcm.len() * 2, &mut error) < 0 {
                        break;
                    }
                }
                pa_drain(handle, &mut error);
                pa_free(handle);
            }
        })
        .map_err(|e| format!("cannot start the sound thread: {e}"))?;
    match rx.recv() {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err("the sound thread ended before it started".to_string()),
    }
}

#[cfg(windows)]
fn start(mixer: Arc<Mutex<Mixer>>, stop: Arc<AtomicBool>) -> Result<std::thread::JoinHandle<()>, String> {
    use crate::mixer::RATE;
    use std::ffi::c_void;
    use std::sync::mpsc;
    use std::time::Duration;

    #[repr(C)]
    struct WaveFormatEx {
        format_tag: u16,
        channels: u16,
        samples_per_sec: u32,
        avg_bytes_per_sec: u32,
        block_align: u16,
        bits_per_sample: u16,
        size: u16,
    }

    #[repr(C)]
    struct WaveHdr {
        data: *mut u8,
        buffer_length: u32,
        bytes_recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut WaveHdr,
        reserved: usize,
    }

    #[link(name = "winmm")]
    extern "system" {
        fn waveOutOpen(handle: *mut *mut c_void, device: u32, format: *const WaveFormatEx, callback: usize, instance: usize, flags: u32) -> u32;
        fn waveOutPrepareHeader(handle: *mut c_void, header: *mut WaveHdr, size: u32) -> u32;
        fn waveOutUnprepareHeader(handle: *mut c_void, header: *mut WaveHdr, size: u32) -> u32;
        fn waveOutWrite(handle: *mut c_void, header: *mut WaveHdr, size: u32) -> u32;
        fn waveOutReset(handle: *mut c_void) -> u32;
        fn waveOutClose(handle: *mut c_void) -> u32;
    }

    const WAVE_MAPPER: u32 = 0xFFFF_FFFF;
    const WHDR_DONE: u32 = 1;
    const BUFFERS: usize = 4;
    const FRAMES: usize = 512;

    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let thread = std::thread::Builder::new()
        .name("lunaris-audio".to_string())
        .spawn(move || {
            // SAFETY: the device handle and the headers belong to this thread alone; the buffers and headers live in vectors that are never
            // resized, so the addresses the driver was given stay valid until the headers are unprepared below, before they are dropped.
            unsafe {
                let format = WaveFormatEx { format_tag: 1, channels: 2, samples_per_sec: RATE, avg_bytes_per_sec: RATE * 4, block_align: 4, bits_per_sample: 16, size: 0 };
                let mut handle: *mut c_void = std::ptr::null_mut();
                let r = waveOutOpen(&mut handle, WAVE_MAPPER, &format, 0, 0, 0);
                if r != 0 {
                    let _ = tx.send(Err(format!("waveOutOpen failed with code {r}: is there a sound device?")));
                    return;
                }
                let mut buffers: Vec<Vec<i16>> = (0..BUFFERS).map(|_| vec![0i16; FRAMES * 2]).collect();
                let mut headers: Vec<WaveHdr> = buffers
                    .iter_mut()
                    .map(|b| WaveHdr { data: b.as_mut_ptr() as *mut u8, buffer_length: (FRAMES * 4) as u32, bytes_recorded: 0, user: 0, flags: 0, loops: 0, next: std::ptr::null_mut(), reserved: 0 })
                    .collect();
                let size = std::mem::size_of::<WaveHdr>() as u32;
                let mut prepared = 0;
                for h in headers.iter_mut() {
                    if waveOutPrepareHeader(handle, h, size) != 0 {
                        break;
                    }
                    prepared += 1;
                }
                if prepared < BUFFERS {
                    for h in headers.iter_mut().take(prepared) {
                        waveOutUnprepareHeader(handle, h, size);
                    }
                    waveOutClose(handle);
                    let _ = tx.send(Err("waveOutPrepareHeader failed".to_string()));
                    return;
                }
                let _ = tx.send(Ok(()));
                let mut queued = [false; BUFFERS];
                let mut scratch = vec![0.0f32; FRAMES * 2];
                while !stop.load(Ordering::SeqCst) {
                    for i in 0..BUFFERS {
                        let flags = std::ptr::read_volatile(&headers[i].flags);
                        if queued[i] && flags & WHDR_DONE == 0 {
                            continue;
                        }
                        {
                            let mut m = mixer.lock().unwrap_or_else(|p| p.into_inner());
                            m.render(&mut scratch);
                        }
                        for (dst, src) in buffers[i].iter_mut().zip(scratch.iter()) {
                            *dst = (src.clamp(-1.0, 1.0) * 32767.0) as i16;
                        }
                        if waveOutWrite(handle, &mut headers[i], size) == 0 {
                            queued[i] = true;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(3));
                }
                waveOutReset(handle);
                for h in headers.iter_mut() {
                    waveOutUnprepareHeader(handle, h, size);
                }
                waveOutClose(handle);
            }
        })
        .map_err(|e| format!("cannot start the sound thread: {e}"))?;
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(thread),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err("the sound device did not answer within 5 seconds".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{Params, RATE};
    use crate::wav::Clip;

    /// Plays 0.2 s of silence through the real device and checks that the device consumed it. Where there is no sound device (a server) the
    /// test is a no-op; nothing audible is played either way.
    #[test]
    fn the_device_pulls_what_the_mixer_renders() {
        let mixer = Arc::new(Mutex::new(Mixer::new()));
        let out = match Output::open(mixer.clone()) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("no sound device here ({e}): skipped");
                return;
            }
        };
        mixer.lock().unwrap().play(Arc::new(Clip::mono(RATE, vec![0.0; RATE as usize / 5])), Params::default()).unwrap();
        let t0 = std::time::Instant::now();
        while mixer.lock().unwrap().active() > 0 && t0.elapsed().as_secs() < 3 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(mixer.lock().unwrap().active(), 0, "the device never asked for the samples");
        assert!(t0.elapsed().as_millis() >= 100, "it cannot have played 0.2 s of sound faster than that: {:?}", t0.elapsed());
        drop(out);
    }
}
