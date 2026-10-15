//! `export_game`: a folder that holds a game and every file it uses, and plays the same from there.
use crate::tests::engine;
use lunaris_json::{parse, Json};
use std::path::Path;

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn write_png(path: &Path, rgb: [u8; 3]) {
    let mut data = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut data, 8, 8);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let pixels: Vec<u8> = (0..64).flat_map(|_| rgb).collect();
        enc.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    std::fs::write(path, data).unwrap();
}

fn hash(e: &mut crate::Engine) -> String {
    e.call("hash", &Json::Null).unwrap().get("hash").unwrap().as_str().unwrap().to_string()
}

fn picture(e: &mut crate::Engine) -> String {
    e.call("render", &j(r#"{"width":64,"height":40,"image":false,"quality":"reference"}"#)).unwrap().get("hash").unwrap().as_str().unwrap().to_string()
}

fn names(r: &Json, key: &str) -> Vec<String> {
    r.get(key).unwrap().as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

#[test]
fn an_exported_game_holds_what_it_uses_and_plays_the_same_from_its_own_folder() {
    let mut e = engine();
    let root = e.root.clone();
    let _ = std::fs::remove_dir_all(root.join("dist"));
    for d in ["art", "fonts", "lang"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    write_png(&root.join("art/hero.png"), [200, 60, 40]);
    write_png(&root.join("art/unused.png"), [0, 0, 0]);
    write_png(&root.join("art/tri.png"), [10, 200, 10]);
    std::fs::write(root.join("art/tri.bin"), [0u8; 12]).unwrap();
    std::fs::write(root.join("art/tri.gltf"), r#"{"asset":{"version":"2.0"},"buffers":[{"uri":"tri.bin","byteLength":12}],"images":[{"uri":"tri.png"},{"uri":"data:image/png;base64,AAAA"}]}"#).unwrap();
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../lunaris-render/testdata/pixel.ttf"), root.join("fonts/ui.ttf")).unwrap();
    std::fs::write(root.join("lang/en.json"), r#"{"hi":"Hello"}"#).unwrap();
    std::fs::write(root.join("lang/de.json"), r#"{"hi":"Hallo"}"#).unwrap();
    for (uid, comps) in [
        (1, r#"{"Transform":{"pos":[0,0,3]},"Camera":{}}"#),
        (2, r#"{"Transform":{"pos":[0,0,0]},"MeshRenderer":{"mesh":"cube","texture":"art/hero.png"}}"#),
        (3, r#"{"Text":{"value":"{#hi}","pos":[0.1,0.1],"size":0.2},"TextFont":{"file":"fonts/ui.ttf"}}"#),
        (4, r#"{"Localization":{"language":"en"}}"#),
        // a model that is only named here (its pictures and buffer must come along), and a file that is named and not there
        (5, r#"{"Name":{"value":"art/tri.gltf"}}"#),
        (6, r#"{"Name":{"value":"art/gone.png"}}"#),
        (7, r#"{"Name":{"value":"not a path.png and some words"}}"#),
    ] {
        e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{comps}}}"#))).unwrap();
    }
    e.call("set_resource", &j(r#"{"name":"score","value":3}"#)).unwrap();
    let (state, frame) = (hash(&mut e), picture(&mut e));

    let r = e.call("export_game", &j(r#"{"out":"dist/mygame","name":"mygame"}"#)).unwrap();
    assert_eq!(r.get("scene").unwrap().as_str(), Some("mygame.lunaris.json"));
    assert_eq!(r.get("state_hash").unwrap().as_str(), Some(state.as_str()));
    let missing = names(&r, "missing");
    assert_eq!(missing, vec!["art/gone.png".to_string()], "only the file that is named and not there");
    let out = root.join("dist/mygame");
    let manifest = parse(&std::fs::read_to_string(out.join("export.json")).unwrap()).unwrap();
    let files: Vec<String> = manifest.get("files").unwrap().as_array().unwrap().iter().map(|f| f.get("path").unwrap().as_str().unwrap().to_string()).collect();
    for want in ["art/hero.png", "art/tri.gltf", "art/tri.bin", "art/tri.png", "fonts/ui.ttf", "lang/en.json", "lang/de.json"] {
        assert!(files.iter().any(|f| f == want), "{want} is exported: {files:?}");
        assert!(out.join(want).is_file(), "{want} is in the folder");
    }
    assert!(!files.iter().any(|f| f == "art/unused.png") && !out.join("art/unused.png").exists(), "a file nothing names stays out");
    for f in ["mygame.lunaris.json", "play.bat", "play.sh", "README.txt"] {
        assert!(out.join(f).is_file(), "{f}");
    }
    assert!(std::fs::read_to_string(out.join("play.bat")).unwrap().contains("play mygame.lunaris.json"));
    assert!(r.get("player").unwrap().is_null(), "this process is not the player program, so it is not copied");

    // the game plays the same from its own folder: same world, same picture (the picture needs the texture, the font and the language file)
    let mut copy = crate::Engine::new(42, out.clone());
    copy.call("load_scene", &j(r#"{"path":"mygame.lunaris.json"}"#)).unwrap();
    assert_eq!(hash(&mut copy), state);
    assert_eq!(picture(&mut copy), frame);
    let lines = copy.call("check", &Json::Null).unwrap();
    assert!(lines.get("problems").unwrap().as_array().unwrap().iter().all(|p| p.get("severity").unwrap().as_str() != Some("error")), "{lines:?}");

    // the workspace is as it was, a folder with files in it is not written to unless asked, and bad folders are refused
    assert_eq!(hash(&mut e), state);
    assert_eq!(e.call("export_game", &j(r#"{"out":"dist/mygame","name":"mygame"}"#)).unwrap_err().code, "folder_not_empty");
    e.call("export_game", &j(r#"{"out":"dist/mygame","name":"mygame","overwrite":true}"#)).unwrap();
    for bad in [r#"{"out":"."}"#, r#"{"out":"../x"}"#, r#"{"out":"/tmp/x"}"#, r#"{"name":"a b"}"#, r#"{"overwrite":"yes"}"#] {
        let err = e.call("export_game", &j(bad)).unwrap_err();
        assert!(["bad_path", "bad_name", "type_mismatch"].contains(&&*err.code), "{bad}: {err}");
    }
}

#[test]
fn a_game_with_errors_is_not_exported_unless_forced() {
    let mut e = engine();
    let _ = std::fs::remove_dir_all(e.root.join("dist"));
    e.call("spawn", &j(r#"{"components":{"Text":{"value":"x"},"TextFont":{"file":"nope.ttf"}}}"#)).unwrap();
    let err = e.call("export_game", &j(r#"{"out":"dist/broken"}"#)).unwrap_err();
    assert_eq!(err.code, "game_has_errors");
    assert!(!e.root.join("dist/broken").exists(), "nothing was written");
    let r = e.call("export_game", &j(r#"{"out":"dist/broken","force":true}"#)).unwrap();
    assert_eq!(r.get("errors_ignored").unwrap().as_u64(), Some(1));
    assert!(e.root.join("dist/broken/game.lunaris.json").is_file());
}
