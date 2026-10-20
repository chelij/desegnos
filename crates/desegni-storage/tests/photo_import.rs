use desegni_storage::collect_identity_source;
use std::fs;

fn jpeg(path: &std::path::Path, width: u32, height: u32) {
    let picture = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x * y) % 239) as u8])
    });
    picture
        .save_with_format(path, image::ImageFormat::Jpeg)
        .unwrap();
}

#[test]
fn a_folder_of_photos_becomes_small_project_photos_with_thumbnails() {
    let root = std::env::temp_dir().join(format!("desegnos-photos-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("work")).unwrap();
    jpeg(&root.join("work/robot-arm.jpg"), 2000, 1400);
    fs::copy(
        root.join("work/robot-arm.jpg"),
        root.join("work/robot-arm-copy.jpg"),
    )
    .unwrap();
    jpeg(&root.join("tiny.jpg"), 64, 64);
    jpeg(&root.join("logo.jpg"), 400, 400);
    fs::write(
        root.join("index.html"),
        "<h1>Cheli</h1><a href='/projects/robot'>Robot arm</a>",
    )
    .unwrap();
    let source = collect_identity_source(&root).unwrap();
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        source.photos.len(),
        1,
        "duplicates, tiny pictures and logos are not photos: {:?}",
        source.skipped
    );
    let photo = &source.photos[0].photo;
    assert!(photo.name.starts_with("work/robot-arm"));
    assert!(photo.width <= 1280 && photo.width >= 480, "{}", photo.width);
    assert_eq!(photo.height, photo.width * 7 / 10, "keeps its shape");
    photo.validate().unwrap();
    assert!(
        source.photos[0]
            .thumbnail
            .starts_with("data:image/jpeg;base64,")
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains("same photo"))
    );
    assert!(
        source
            .skipped
            .iter()
            .any(|reason| reason.contains("too small"))
    );
    assert!(
        source
            .files
            .iter()
            .any(|file| file.name == "logo.jpg" && file.asset.is_some())
    );
    assert!(source.files.iter().any(|file| file.name == "index.html"));
}
