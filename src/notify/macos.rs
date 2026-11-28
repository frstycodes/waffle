// Notifications through UNUserNotificationCenter, posted as Waffle's bundle with its name and icon. A bare
// binary has no bundle to post as (UNUserNotificationCenter raises on one), so `cargo run` wraps the build
// in a dev bundle first (tools/macos-run.sh).
use std::{cell::OnceCell, sync::Once};

use block2::{DynBlock, RcBlock};
use objc2::{
    AllocAnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

define_class!(
    // Shows banners while Waffle is in front (macOS hides them by default) and routes clicks back to it.
    #[unsafe(super(NSObject))]
    #[name = "WaffleNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(&self, _: &UNUserNotificationCenter, _: &UNNotification, done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
            done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &DynBlock<dyn Fn()>) {
            if let Some(link) = response.notification().request().content().targetContentIdentifier() {
                crate::notify::clicked(link.to_string());
            }
            done.call(());
        }
    }
);

thread_local! {
    // The center holds its delegate weakly.
    static DELEGATE: OnceCell<Retained<Delegate>> = const { OnceCell::new() };
}

// Takes clicks from launch on, so one that started Waffle still opens its channel.
pub fn install() {
    let Some(center) = center() else { return };
    DELEGATE.with(|cell| {
        // SAFETY: a plain NSObject subclass with no ivars.
        let delegate = cell.get_or_init(|| unsafe { msg_send![Delegate::alloc(), init] });
        center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
    });
}

// Asks for permission the first time (later calls answer at once), then posts.
pub fn post(title: &str, body: &str, link: String) {
    let Some(center) = center() else { return };
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setTargetContentIdentifier(Some(&NSString::from_str(&link)));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&link), &content, None);
    let poster = center.clone();
    let then_post = RcBlock::new(move |granted: Bool, _: *mut NSError| {
        if granted.as_bool() {
            poster.addNotificationRequest_withCompletionHandler(&request, None);
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Badge, &then_post);
}

fn center() -> Option<Retained<UNUserNotificationCenter>> {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        static WARN: Once = Once::new();
        WARN.call_once(|| eprintln!("[notify] not running from an app bundle, so macOS has nothing to post notifications as"));
        return None;
    }
    Some(UNUserNotificationCenter::currentNotificationCenter())
}
