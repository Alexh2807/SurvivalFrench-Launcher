//! Fichiers de l'interface, intégrés à l'exe (ou lus sur le disque avec --ui-dir, pour les retoucher sans recompiler).
use std::borrow::Cow;

pub const LOGO: &[u8] = include_bytes!("../ui/logo.png");

const FILES: &[(&str, &[u8], &str)] = &[
    ("index.html", include_bytes!("../ui/index.html"), "text/html; charset=utf-8"),
    ("style.css", include_bytes!("../ui/style.css"), "text/css; charset=utf-8"),
    ("app.js", include_bytes!("../ui/app.js"), "text/javascript; charset=utf-8"),
    ("scene.js", include_bytes!("../ui/scene.js"), "text/javascript; charset=utf-8"),
    ("skin.js", include_bytes!("../ui/skin.js"), "text/javascript; charset=utf-8"),
    ("logo.png", LOGO, "image/png"),
    ("fonts/ubuntu-light.woff", include_bytes!("../ui/fonts/ubuntu-light.woff"), "font/woff"),
    ("fonts/ubuntu-regular.woff", include_bytes!("../ui/fonts/ubuntu-regular.woff"), "font/woff"),
    ("fonts/ubuntu-medium.woff", include_bytes!("../ui/fonts/ubuntu-medium.woff"), "font/woff"),
    ("fonts/ubuntu-bold.woff", include_bytes!("../ui/fonts/ubuntu-bold.woff"), "font/woff"),
];

pub fn file(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let path = if path.is_empty() { "index.html" } else { path };
    let (name, data, mime) = FILES.iter().find(|(n, _, _)| *n == path)?;
    if let Some(dir) = crate::base::arg_value("--ui-dir") {
        if let Ok(b) = std::fs::read(std::path::Path::new(&dir).join(name)) {
            return Some((Cow::Owned(b), mime));
        }
    }
    Some((Cow::Borrowed(*data), mime))
}
