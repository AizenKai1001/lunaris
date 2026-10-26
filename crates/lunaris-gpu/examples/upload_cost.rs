//! What it costs to put bytes on the card the way the renderer puts a mesh there (a buffer made, then `queue.write_buffer`), alone and followed by a submit: `cargo run --release -p lunaris-gpu --example upload_cost`
//! (`LUNARIS_GPU=name` picks the adapter). Prints the milliseconds a call takes for a few sizes, to tell a slow copy from a slow allocation.
use lunaris_gpu::gpu::Gpu;
use std::time::Instant;

fn main() {
    let gpu = Gpu::headless().expect("a graphics adapter");
    println!("adapter: {}", gpu.name());
    let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
    for kb in [64usize, 330, 1024, 6144] {
        let data = vec![7u8; kb * 1024];
        let runs = 40;
        let (mut make, mut write, mut flush) = (0.0f64, 0.0f64, 0.0f64);
        for _ in 0..runs {
            let t = Instant::now();
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage, mapped_at_creation: false });
            make += t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            gpu.queue.write_buffer(&buf, 0, &data);
            write += t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            gpu.queue.submit([]);
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            flush += t.elapsed().as_secs_f64() * 1000.0;
            drop(buf);
        }
        let n = runs as f64;
        println!("{kb:>5} KB: create_buffer {:.3} ms, write_buffer {:.3} ms, submit + wait {:.3} ms (means of {runs})", make / n, write / n, flush / n);
        // the same sizes written into ONE buffer many times (no allocation of the destination)
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage, mapped_at_creation: false });
        let t = Instant::now();
        for _ in 0..runs {
            gpu.queue.write_buffer(&buf, 0, &data);
        }
        gpu.queue.submit([]);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        println!("        the same buffer written {runs} times, one submit at the end: {:.3} ms a write", t.elapsed().as_secs_f64() * 1000.0 / n);
    }
    // many buffers, one write each, as a city streaming in does it: the pending writes pile up until a submit, so how often the submit comes matters
    for (every, label) in [(0usize, "no submit until the end"), (64, "a submit every 64 writes"), (8, "a submit every 8 writes"), (1, "a submit after every write")] {
        let data = vec![7u8; 330 * 1024];
        let count = 1500;
        let mut bufs = Vec::with_capacity(count);
        let t = Instant::now();
        for i in 0..count {
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage, mapped_at_creation: false });
            gpu.queue.write_buffer(&buf, 0, &data);
            bufs.push(buf);
            if every > 0 && (i + 1) % every == 0 {
                gpu.queue.submit([]);
            }
        }
        let writes = t.elapsed().as_secs_f64() * 1000.0;
        gpu.queue.submit([]);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        println!("{count} buffers of 330 KB, {label}: {:.3} ms a buffer in the loop, {:.0} ms with the last wait", writes / count as f64, t.elapsed().as_secs_f64() * 1000.0);
        drop(bufs);
    }
    // where the cost of the loop above comes from: the allocation of a buffer, the copy, or the number of live buffers (one big buffer written in slices is what an arena would do)
    let data = vec![7u8; 330 * 1024];
    let count = 1500;
    let t = Instant::now();
    for _ in 0..count {
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage, mapped_at_creation: false });
        gpu.queue.write_buffer(&buf, 0, &data);
        drop(buf);
    }
    gpu.queue.submit([]);
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    println!("A: {count} buffers made, written and dropped at once: {:.3} ms a buffer", t.elapsed().as_secs_f64() * 1000.0 / count as f64);
    let t = Instant::now();
    let kept: Vec<wgpu::Buffer> = (0..count).map(|_| gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage, mapped_at_creation: false })).collect();
    println!("B: {count} buffers of 330 KB only created and kept: {:.3} ms a buffer", t.elapsed().as_secs_f64() * 1000.0 / count as f64);
    let t = Instant::now();
    for b in &kept {
        gpu.queue.write_buffer(b, 0, &data);
    }
    gpu.queue.submit([]);
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    println!("B2: the same {count} buffers then written (one submit, one wait): {:.3} ms a buffer", t.elapsed().as_secs_f64() * 1000.0 / count as f64);
    drop(kept);
    // an arena: one buffer (within the 256 MB a buffer may be) written in slices of 330 KB
    let slices = 380;
    let big = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("arena"), size: (data.len() * slices) as u64, usage, mapped_at_creation: false });
    let t = Instant::now();
    for i in 0..slices {
        gpu.queue.write_buffer(&big, (i * data.len()) as u64, &data);
    }
    gpu.queue.submit([]);
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    println!("D: one {} MB buffer written in {slices} slices of 330 KB (an arena): {:.3} ms a slice ({} errors reported)", data.len() * slices / 1048576, t.elapsed().as_secs_f64() * 1000.0 / slices as f64, gpu.take_errors().len());
    // does the cost of a new buffer depend on its size?
    for kb in [4usize, 16, 64, 330, 1024, 6144] {
        let data = vec![7u8; kb * 1024];
        let count = 300;
        let t = Instant::now();
        for _ in 0..count {
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage, mapped_at_creation: false });
            gpu.queue.write_buffer(&buf, 0, &data);
            drop(buf);
        }
        gpu.queue.submit([]);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        println!("E: {count} new buffers of {kb} KB made, written and dropped, no flush in between: {:.3} ms a buffer", t.elapsed().as_secs_f64() * 1000.0 / count as f64);
    }
    // the other way into a new buffer: mapped at creation
    let data = vec![7u8; 330 * 1024];
    let t = Instant::now();
    let mut kept = Vec::new();
    for _ in 0..300 {
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size: data.len() as u64, usage: wgpu::BufferUsages::VERTEX, mapped_at_creation: true });
        buf.slice(..).get_mapped_range_mut().expect("mapped at creation").copy_from_slice(&data);
        buf.unmap();
        kept.push(buf);
    }
    gpu.queue.submit([]);
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    println!("F: 300 buffers of 330 KB mapped at creation, filled, unmapped: {:.3} ms a buffer", t.elapsed().as_secs_f64() * 1000.0 / 300.0);
}
