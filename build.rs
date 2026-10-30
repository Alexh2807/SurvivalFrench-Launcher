// Icône et manifeste Windows (DPI par écran, thèmes modernes) intégrés à l'exe.
fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=icon.ico");
    embed_resource::compile("res/app.rc", embed_resource::NONE).manifest_required().unwrap();
}
