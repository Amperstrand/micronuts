//! Browser camera QR scanning (wasm): getUserMedia preview overlaid on
//! the wallet canvas + a slint::Timer decode loop feeding
//! [`crate::qr_decode::decode_rgba`]. The GM65 module decodes QR
//! on-chip, so this software path exists only on the web.

use std::cell::RefCell;

use slint::{ComponentHandle, Weak};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::qr_decode::decode_rgba;
use crate::ui::{MainWindow, Page, WalletLogic};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 240;
/// Preview height in CSS pixels; the wallet canvas is 800 tall, so the
/// bottom 280px (status + Cancel) stay visible under the overlay.
const OVERLAY_HEIGHT_PX: u32 = 520;

struct CameraState {
    video: web_sys::HtmlVideoElement,
    stream: web_sys::MediaStream,
    timer: slint::Timer,
}

thread_local! {
    static CAMERA: RefCell<Option<CameraState>> = const { RefCell::new(None) };
}

fn set_scan_status(weak: &Weak<MainWindow>, status: String) {
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.global::<WalletLogic>().set_scan_status(status.into());
    });
}

/// Start the camera and decode loop; status flows through the
/// WalletLogic `scan-status` global. A successful decode stops the
/// camera, fills `token-in`, and navigates back to Receive.
pub fn start_camera(weak: Weak<MainWindow>) {
    stop_camera();
    let Some(window) = web_sys::window() else {
        set_scan_status(&weak, String::from("camera: no window"));
        return;
    };
    let Ok(devices) = window.navigator().media_devices() else {
        set_scan_status(&weak, String::from("camera: unavailable in this browser"));
        return;
    };

    let constraints = web_sys::MediaStreamConstraints::new();
    let video_constraints = web_sys::MediaTrackConstraints::new();
    video_constraints.set_facing_mode(&wasm_bindgen::JsValue::from_str("environment"));
    constraints.set_video(&video_constraints);

    let document = window.document().expect("document");
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .expect("create canvas")
        .dyn_into()
        .expect("canvas element");
    canvas.set_width(FRAME_WIDTH);
    canvas.set_height(FRAME_HEIGHT);

    let weak_for_future = weak.clone();
    let future = async move {
        let Ok(promise) = devices.get_user_media_with_constraints(&constraints) else {
            set_scan_status(
                &weak_for_future,
                String::from("camera unavailable (permission denied or no camera)"),
            );
            return;
        };
        let stream = match JsFuture::from(promise).await {
            Ok(value) => value
                .dyn_into::<web_sys::MediaStream>()
                .expect("media stream"),
            Err(_) => {
                set_scan_status(
                    &weak_for_future,
                    String::from("camera unavailable (permission denied or no camera)"),
                );
                return;
            }
        };

        let video: web_sys::HtmlVideoElement = document
            .create_element("video")
            .expect("create video")
            .dyn_into()
            .expect("video element");
        video.set_autoplay(true);
        video.set_muted(true);
        video.set_attribute("playsinline", "").expect("playsinline");
        let style = video.style();
        let _ = style.set_property("position", "fixed");
        let _ = style.set_property("left", "50%");
        let _ = style.set_property("transform", "translateX(-50%)");
        let _ = style.set_property("top", "0");
        let _ = style.set_property("width", "480px");
        let _ = style.set_property("height", &format!("{OVERLAY_HEIGHT_PX}px"));
        let _ = style.set_property("object-fit", "cover");
        let _ = style.set_property("z-index", "10");
        let _ = style.set_property("background", "#000");
        video.set_src_object(Some(&stream));
        let _ = document.body().expect("body").append_child(&video);
        if let Ok(play) = video.play() {
            let _ = JsFuture::from(play).await;
        }

        let timer = slint::Timer::default();
        let weak_for_frames = weak_for_future.clone();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(125),
            move || capture_frame(&weak_for_frames, &canvas),
        );

        CAMERA.with(|slot| {
            *slot.borrow_mut() = Some(CameraState {
                video,
                stream,
                timer,
            });
        });
        set_scan_status(
            &weak_for_future,
            String::from("scanning — point at a QR code"),
        );
    };
    wasm_bindgen_futures::spawn_local(future);
}

/// Stop the camera: halt the decode timer, stop tracks, remove the
/// preview element.
pub fn stop_camera() {
    CAMERA.with(|slot| {
        if let Some(state) = slot.borrow_mut().take() {
            state.timer.stop();
            for track in state.stream.get_tracks().iter() {
                if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
                    track.stop();
                }
            }
            if let Some(parent) = state.video.parent_element() {
                let _ = parent.remove_child(&state.video);
            }
        }
    });
}

fn capture_frame(weak: &Weak<MainWindow>, canvas: &web_sys::HtmlCanvasElement) {
    let decoded = CAMERA.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return None;
        };
        if state.video.video_width() == 0 {
            return None;
        }
        let Ok(Some(context)) = canvas.get_context("2d") else {
            return None;
        };
        let Ok(context) = context.dyn_into::<web_sys::CanvasRenderingContext2d>() else {
            return None;
        };
        let drew = context
            .draw_image_with_html_video_element_and_dw_and_dh(
                &state.video,
                0.0,
                0.0,
                f64::from(FRAME_WIDTH),
                f64::from(FRAME_HEIGHT),
            )
            .is_ok();
        if !drew {
            return None;
        }
        let image = context
            .get_image_data(0.0, 0.0, f64::from(FRAME_WIDTH), f64::from(FRAME_HEIGHT))
            .ok()?;
        decode_rgba(FRAME_WIDTH as usize, FRAME_HEIGHT as usize, &image.data().0)
    });

    if let Some(text) = decoded {
        stop_camera();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let logic = ui.global::<WalletLogic>();
            logic.set_scanning(false);
            logic.set_scan_status(String::new().into());
            logic.set_token_in(text.into());
            ui.invoke_navigate(Page::Receive);
        });
    }
}
