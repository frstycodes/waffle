// The fonts Waffle draws with, built into the binary.
use std::borrow::Cow;

pub fn bundled() -> Vec<Cow<'static, [u8]>> {
    [
        include_bytes!("../assets/fonts/Gabarito-Regular.ttf").as_slice(),
        include_bytes!("../assets/fonts/Gabarito-Medium.ttf"),
        include_bytes!("../assets/fonts/Gabarito-SemiBold.ttf"),
        include_bytes!("../assets/fonts/Gabarito-Bold.ttf"),
        include_bytes!("../assets/fonts/Gabarito-ExtraBold.ttf"),
        include_bytes!("../assets/fonts/Figtree-Regular.ttf"),
        include_bytes!("../assets/fonts/Figtree-Medium.ttf"),
        include_bytes!("../assets/fonts/Figtree-SemiBold.ttf"),
        include_bytes!("../assets/fonts/Figtree-Bold.ttf"),
        include_bytes!("../assets/fonts/RobotoMono-Regular.ttf"),
        include_bytes!("../assets/fonts/RobotoMono-Bold.ttf"),
    ]
    .map(Cow::Borrowed)
    .to_vec()
}
