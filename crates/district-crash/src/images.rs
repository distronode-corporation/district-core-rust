//! The debug images a report lists, so the service can match the raw
//! addresses of a stripped release build to the debug files uploaded for it:
//! an ELF image by its build id, a PE image by its PDB's id and age.
//!
//! Only two images, never every library loaded: the main executable, and the
//! image this crate's own code is linked into. On Linux both are the app's
//! executable; on Windows the second is the app's Rust library (the
//! executable is the host that loads it). The app's own code is what its
//! uploaded debug files can symbolicate, and a full list would describe the
//! machine instead (system libraries and their versions, and whatever else is
//! loaded into the process, such as security software or shell extensions),
//! some of it from under the user's home directory.

use std::borrow::Cow;

use sentry::integrations::debug_images::debug_images;
use sentry::protocol::{DebugImage, DebugMeta, Event};
use sentry::{ClientOptions, Integration};

/// Adds the images to every report. The list is read once, when reporting is
/// turned on, so the panic handler reads nothing from the system. Its paths go
/// through the same scrub as the rest of the report.
pub(crate) struct OwnImages(DebugMeta);

impl OwnImages {
    pub(crate) fn new() -> Self {
        let main = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let here = OwnImages::new as fn() -> Self as usize as u64;
        Self(DebugMeta {
            images: own_images(debug_images(), &main, here, std::env::consts::ARCH),
            ..Default::default()
        })
    }
}

impl Integration for OwnImages {
    fn name(&self) -> &'static str {
        "district-crash-images"
    }

    fn process_event(
        &self,
        mut event: Event<'static>,
        _: &ClientOptions,
    ) -> Option<Event<'static>> {
        event.debug_meta = Cow::Owned(self.0.clone());
        Some(event)
    }
}

/// Of `images`, the one named `main` and the one holding `address`, with the
/// architecture set (Sentry's integration leaves it empty).
fn own_images(images: Vec<DebugImage>, main: &str, address: u64, arch: &str) -> Vec<DebugImage> {
    images
        .into_iter()
        .filter_map(|image| match image {
            DebugImage::Symbolic(mut image) => {
                let start = image.image_addr.0;
                let holds = start <= address && address - start < image.image_size;
                image.arch = Some(sentry_arch(arch).to_owned());
                (holds || image.name == main).then_some(DebugImage::Symbolic(image))
            }
            _ => None,
        })
        .collect()
}

/// Rust's name for the architecture, as Sentry spells it.
fn sentry_arch(rust: &str) -> &str {
    match rust {
        "aarch64" => "arm64",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use sentry::protocol::{ProguardDebugImage, SymbolicDebugImage};
    use sentry::types::{DebugId, Uuid};

    use super::*;

    fn image(name: &str, addr: u64, size: u64) -> DebugImage {
        SymbolicDebugImage {
            name: name.to_owned(),
            arch: None,
            image_addr: addr.into(),
            image_size: size,
            image_vmaddr: 0.into(),
            id: DebugId::nil(),
            code_id: None,
            debug_file: None,
        }
        .into()
    }

    /// Each image's file and architecture, read as the report would send
    /// them.
    fn names(images: &[DebugImage]) -> Vec<(String, String)> {
        images
            .iter()
            .map(|image| serde_json::to_value(image).unwrap())
            .map(|image| (image["name"].to_string(), image["arch"].to_string()))
            .collect()
    }

    fn pair(file: &str, arch: &str) -> (String, String) {
        (format!("{file:?}"), format!("{arch:?}"))
    }

    #[test]
    fn the_main_executable_and_the_image_holding_this_code_only() {
        let images = vec![
            image("/usr/bin/app", 0x1000, 0x1000),
            image("/usr/lib/libc.so.6", 0x8000, 0x1000),
            image("/opt/app/libapp_ffi.so", 0x10_000, 0x1000),
            image("/usr/lib/libgtk.so", 0x20_000, 0x1000),
            ProguardDebugImage { uuid: Uuid::nil() }.into(),
        ];

        let kept = own_images(images, "/usr/bin/app", 0x10_800, "x86_64");

        assert_eq!(
            names(&kept),
            [
                pair("/usr/bin/app", "x86_64"),
                pair("/opt/app/libapp_ffi.so", "x86_64"),
            ]
        );
    }

    #[test]
    fn an_address_just_outside_an_image_is_not_in_it() {
        let images = vec![image("a", 0x1000, 0x1000), image("b", 0x3000, 0x1000)];
        assert_eq!(names(&own_images(images.clone(), "", 0x2000, "x86_64")), []);
        assert_eq!(names(&own_images(images.clone(), "", 0xfff, "x86_64")), []);
        assert_eq!(
            names(&own_images(images, "", 0x1fff, "aarch64")),
            [pair("a", "arm64")]
        );
    }

    #[test]
    fn this_process_lists_the_image_holding_this_code() {
        let here = this_process_lists_the_image_holding_this_code as fn() as usize as u64;
        let OwnImages(meta) = OwnImages::new();
        assert!(
            meta.images.iter().any(|image| matches!(image,
                DebugImage::Symbolic(image)
                    if image.image_addr.0 <= here && here - image.image_addr.0 < image.image_size)),
            "{meta:?}"
        );
        assert_eq!(OwnImages(meta).name(), "district-crash-images");
    }
}
