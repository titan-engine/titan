use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use titan_generated_asset::{
    BUILD_PNG, LazyTile, decode,
    generator::{self, Generator, Inputs, Outcome},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "titan-generated-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lazy_construction_does_no_io_and_second_access_is_memory_only() {
    let directory = Scratch::new();
    let mut lazy = LazyTile::new(&directory.0, Inputs::default());
    assert!(!directory.0.exists());
    assert_eq!(lazy.generator.generation_count, 0);
    let first = lazy.get().unwrap().png.clone();
    assert_eq!(lazy.generator.generation_count, 1);
    fs::remove_dir_all(&directory.0).unwrap();
    assert_eq!(lazy.get().unwrap().png, first);
    assert!(!directory.0.exists());
    assert_eq!(lazy.generator.generation_count, 1);
}

#[test]
fn build_uncached_cached_and_loose_file_images_are_identical() {
    let directory = Scratch::new();
    let inputs = Inputs::default();
    let mut generator = Generator::default();
    let baseline = generator.generate(inputs).unwrap();
    assert_eq!(generator.generate(inputs).unwrap(), baseline);
    let cold = generator::load(&directory.0, inputs, &mut generator).unwrap();
    assert_eq!(cold.outcome, Outcome::Generated);
    let count = generator.generation_count;
    let path = inputs.cache_path(&directory.0);
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let warm = generator::load(&directory.0, inputs, &mut generator).unwrap();
    assert_eq!(warm.outcome, Outcome::Reused);
    assert_eq!(count, generator.generation_count);
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    assert_eq!(baseline, cold.png);
    assert_eq!(baseline, warm.png);
    assert_eq!(baseline, BUILD_PNG);
    let loose = directory.0.join("tile.png");
    fs::write(&loose, &baseline).unwrap();
    assert_eq!(
        decode(&fs::read(loose).unwrap()).unwrap(),
        decode(BUILD_PNG).unwrap()
    );
}

#[test]
fn input_and_version_changes_invalidate_identity() {
    let directory = Scratch::new();
    let mut generator = Generator::default();
    let base = Inputs::default();
    let original = generator::load(&directory.0, base, &mut generator).unwrap();
    for changed in [
        Inputs {
            seed: base.seed + 1,
            ..base
        },
        Inputs {
            version: base.version + 1,
            ..base
        },
    ] {
        assert_ne!(changed.key(), base.key());
        let asset = generator::load(&directory.0, changed, &mut generator).unwrap();
        assert_eq!(asset.outcome, Outcome::Generated);
        assert_eq!(asset.png, generator.generate(changed).unwrap());
        assert_eq!(
            generator::load(&directory.0, changed, &mut generator)
                .unwrap()
                .outcome,
            Outcome::Reused
        );
    }
    assert_eq!(
        generator::load(&directory.0, base, &mut generator)
            .unwrap()
            .png,
        original.png
    );
}

#[test]
fn corrupt_truncated_oversized_wrong_key_and_invalid_png_entries_recover() {
    let directory = Scratch::new();
    let inputs = Inputs::default();
    let mut generator = Generator::default();
    let expected = generator::load(&directory.0, inputs, &mut generator)
        .unwrap()
        .png;
    let path = inputs.cache_path(&directory.0);
    let valid = fs::read(&path).unwrap();
    let mut bad_checksum = valid.clone();
    *bad_checksum.last_mut().unwrap() ^= 1;
    let mut wrong_key = valid.clone();
    wrong_key[8] ^= 1;
    let mut invalid_png = valid.clone();
    invalid_png[generator::HEADER_BYTES] ^= 1;
    let hash = generator::checksum(&invalid_png[generator::HEADER_BYTES..]);
    invalid_png[16..24].copy_from_slice(&hash.to_le_bytes());
    for damaged in [
        vec![],
        valid[..10].to_vec(),
        bad_checksum,
        wrong_key,
        invalid_png,
        vec![0; generator::HEADER_BYTES + generator::MAX_PNG_BYTES + 1],
    ] {
        fs::write(&path, damaged).unwrap();
        let count = generator.generation_count;
        let recovered = generator::load(&directory.0, inputs, &mut generator).unwrap();
        assert_eq!(recovered.outcome, Outcome::Recovered);
        assert_eq!(generator.generation_count, count + 1);
        assert_eq!(recovered.png, expected);
        assert_eq!(
            generator::load(&directory.0, inputs, &mut generator)
                .unwrap()
                .outcome,
            Outcome::Reused
        );
    }
}

#[test]
fn concurrent_writers_publish_complete_identical_entries() {
    let directory = Scratch::new();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                barrier.wait();
                let asset =
                    generator::load(&directory.0, Inputs::default(), &mut Generator::default())
                        .unwrap();
                assert_eq!(asset.png, BUILD_PNG);
            });
        }
    });
    assert_eq!(
        generator::load(&directory.0, Inputs::default(), &mut Generator::default())
            .unwrap()
            .outcome,
        Outcome::Reused
    );
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn filesystem_errors_are_reported_and_lazy_access_can_retry() {
    let directory = Scratch::new();
    fs::create_dir_all(&directory.0).unwrap();
    let blocked = directory.0.join("blocked");
    fs::write(&blocked, "not a directory").unwrap();
    let mut lazy = LazyTile::new(&blocked, Inputs::default());
    assert!(lazy.get().is_err());
    fs::remove_file(&blocked).unwrap();
    assert_eq!(lazy.get().unwrap().png, BUILD_PNG);
    assert_eq!(fs::read_dir(blocked).unwrap().count(), 1);
}

#[test]
fn default_tile_has_fixed_rgba_reference() {
    let image = decode(BUILD_PNG).unwrap();
    assert_eq!((image.width(), image.height()), (16, 16));
    assert_eq!(generator::checksum(image.pixels()), 0x65c6af5c946efc09);
    // Independent pixel anchors: transparent border and alternating blue squares.
    let at = |x: usize, y: usize| &image.pixels()[(y * 16 + x) * 4..(y * 16 + x + 1) * 4];
    assert_eq!(at(0, 0), [7, 0, 210, 0]);
    assert_eq!(at(1, 1), [55, 3, 210, 255]);
    assert_eq!(at(4, 1), [106, 6, 90, 255]);
    assert_eq!(at(15, 15), [215, 45, 210, 0]);
}
