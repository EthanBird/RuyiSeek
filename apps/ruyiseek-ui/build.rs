fn main() {
    slint_build::compile("../../ui/launcher.slint").expect("the RuyiSeek desktop UI must compile");
    slint_build::compile("../../ui/context-menu.slint")
        .expect("the RuyiSeek context menu UI must compile");
}
