extern crate alloc;

use std::io::{self, Read, Write};
use std::sync::mpsc;
use std::thread;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::Pixel;
use rand::RngCore;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::Sdl;

use micronuts_app::display::{HEIGHT, WIDTH};
use micronuts_app::hardware::{MicronutsHardware, ScanError, Scanner, TouchPoint};
use micronuts_app::protocol::{Frame, FrameDecoder, Response, MAX_PAYLOAD_SIZE};

fn rgb565_to_raw(color: Rgb565) -> u16 {
    let r = color.r();
    let g = color.g();
    let b = color.b();
    ((r & 0xF8) as u16) << 8 | ((g & 0xFC) as u16) << 3 | ((b & 0xF8) as u16) >> 3
}

struct Sdl2Display {
    pixels: Vec<u16>,
    sdl: Sdl,
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    texture: sdl2::render::Texture<'static>,
    dirty: bool,
}

impl Sdl2Display {
    fn new() -> Self {
        let sdl_context = sdl2::init().expect("Failed to init SDL2");
        let video_subsystem = sdl_context.video().expect("Failed to init video subsystem");

        let window = video_subsystem
            .window("Micronuts Simulator", WIDTH as u32, HEIGHT as u32)
            .position_centered()
            .build()
            .expect("Failed to create window");

        let canvas = window
            .into_canvas()
            .build()
            .expect("Failed to create canvas");

        let texture_creator = canvas.texture_creator();
        let texture = unsafe {
            let creator: &'static sdl2::render::TextureCreator<sdl2::video::WindowContext> =
                core::mem::transmute(&texture_creator);
            creator
                .create_texture_streaming(PixelFormatEnum::RGB565, WIDTH as u32, HEIGHT as u32)
                .expect("Failed to create texture")
        };

        let pixels = vec![0u16; (WIDTH * HEIGHT) as usize];

        Sdl2Display {
            pixels,
            sdl: sdl_context,
            canvas,
            texture,
            dirty: true,
        }
    }

    fn present(&mut self) {
        if !self.dirty {
            return;
        }

        let width = WIDTH as usize;
        let raw_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(self.pixels.as_ptr() as *const u8, self.pixels.len() * 2)
        };

        self.texture.update(None, raw_bytes, width * 2).ok();

        self.canvas.clear();
        self.canvas
            .copy(&self.texture, None, None)
            .expect("Failed to copy texture");
        self.canvas.present();
        self.dirty = false;
    }

    fn poll_events(&mut self) -> Option<TouchPoint> {
        let mut event_pump = self.sdl.event_pump().expect("event pump");
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    std::process::exit(0);
                }
                Event::MouseButtonDown { x, y, .. } => {
                    return Some(TouchPoint {
                        x: x as u16,
                        y: y as u16,
                        detected: true,
                    });
                }
                Event::MouseButtonUp { .. } => {
                    return Some(TouchPoint {
                        x: 0,
                        y: 0,
                        detected: false,
                    });
                }
                _ => {}
            }
        }
        None
    }
}

impl OriginDimensions for Sdl2Display {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

impl DrawTarget for Sdl2Display {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let x = coord.x as u32;
            let y = coord.y as u32;
            if x < WIDTH && y < HEIGHT {
                self.pixels[(y * WIDTH + x) as usize] = rgb565_to_raw(color);
                self.dirty = true;
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let raw = rgb565_to_raw(color);
        for px in self.pixels.iter_mut() {
            *px = raw;
        }
        self.dirty = true;
        Ok(())
    }
}

struct MockHardware {
    display: Sdl2Display,
    rng: rand::rngs::ThreadRng,
    decoder: FrameDecoder,
    stdin_rx: mpsc::Receiver<Vec<u8>>,
    stdin_buf: Vec<u8>,
    pending_touch: Option<TouchPoint>,
}

impl MockHardware {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut buf = [0u8; 64];
            loop {
                match io::stdin().read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            display: Sdl2Display::new(),
            rng: rand::thread_rng(),
            decoder: FrameDecoder::new(),
            stdin_rx: rx,
            stdin_buf: Vec::new(),
            pending_touch: None,
        }
    }
}

impl MicronutsHardware for MockHardware {
    type Display = Sdl2Display;

    fn display(&mut self) -> &mut Self::Display {
        &mut self.display
    }

    fn rng_fill_bytes(&mut self, dest: &mut [u8]) {
        self.rng.fill_bytes(dest);
    }

    async fn transport_recv_frame(&mut self) -> Option<Frame> {
        loop {
            while let Ok(data) = self.stdin_rx.try_recv() {
                self.stdin_buf.extend_from_slice(&data);
            }

            if let Some(frame) = self.decoder.decode(&self.stdin_buf) {
                self.stdin_buf.clear();
                return Some(frame);
            }

            embassy_time::Timer::after(embassy_time::Duration::from_millis(1)).await;
        }
    }

    async fn transport_send(&mut self, response: &Response) {
        let mut buf = [0u8; MAX_PAYLOAD_SIZE + 3];
        let len = response.encode(&mut buf);
        let _ = io::stdout().write_all(&buf[..len]);
        let _ = io::stdout().flush();
    }

    fn touch_get(&mut self) -> Option<TouchPoint> {
        if let Some(tp) = self.display.poll_events() {
            self.pending_touch = Some(tp);
        }
        let result = self.pending_touch.take();
        if let Some(tp) = result {
            if tp.detected {
                self.pending_touch = Some(tp);
                println!("[TOUCH] x={}, y={}", tp.x, tp.y);
                return Some(tp);
            }
        }
        None
    }

    async fn delay_ms(&mut self, ms: u32) {
        self.display.present();
        embassy_time::Timer::after(embassy_time::Duration::from_millis(ms as u64)).await;
    }
}

impl Scanner for MockHardware {
    async fn trigger(&mut self) -> Result<(), ScanError> {
        println!("[SCANNER] Trigger scan");
        Ok(())
    }

    fn try_read(&mut self) -> Option<alloc::vec::Vec<u8>> {
        None
    }

    async fn read_scan(&mut self) -> Option<alloc::vec::Vec<u8>> {
        None
    }

    async fn stop(&mut self) {
        println!("[SCANNER] Stop scan");
    }

    fn is_connected(&self) -> bool {
        false
    }

    async fn set_aim(&mut self, enabled: bool) -> Result<(), ScanError> {
        println!("[SCANNER] Aim: {}", if enabled { "ON" } else { "OFF" });
        Ok(())
    }

    fn debug_dump_settings(&mut self) {
        println!("[SCANNER] debug_dump_settings (no-op in simulator)");
    }
}

fn has_nvidia_gpu() -> bool {
    if std::env::var("SDL_VIDEODRIVER").is_ok() {
        return false;
    }
    let entries = match std::fs::read_dir("/sys/class/drm") {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let vendor_path = entry.path().join("device/vendor");
        if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
            if vendor.contains("0x10de") {
                return true;
            }
        }
    }
    false
}

fn sdl2_crashes_with_default_driver() -> bool {
    if !has_nvidia_gpu() {
        return false;
    }
    eprintln!("[sim] NVIDIA GPU detected — testing SDL2 video driver...");

    let mut pipe_fds: [libc::c_int; 2] = [0; 2];
    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        return false;
    }

    let pid = unsafe { libc::fork() };
    if pid < 0 {
        unsafe { libc::close(pipe_fds[0]) };
        unsafe { libc::close(pipe_fds[1]) };
        return false;
    }

    if pid == 0 {
        unsafe {
            libc::close(pipe_fds[0]);
            libc::alarm(3);
        }
        let ok = std::panic::catch_unwind(|| {
            let sdl = sdl2::init().expect("init");
            let video = sdl.video().expect("video");
            let _window = video.window("probe", 1, 1).build().expect("window");
        })
        .is_ok();
        let byte: u8 = if ok { 1 } else { 0 };
        unsafe {
            let _ = libc::write(pipe_fds[1], &byte as *const u8 as *const libc::c_void, 1);
            libc::_exit(0);
        }
    }

    unsafe { libc::close(pipe_fds[1]) };

    let read_fd = pipe_fds[0];
    let mut tv = libc::timeval {
        tv_sec: 4,
        tv_usec: 0,
    };
    let mut fds: libc::fd_set = unsafe { std::mem::zeroed() };
    unsafe {
        libc::FD_ZERO(&mut fds);
        libc::FD_SET(read_fd, &mut fds);
    }

    let ready = unsafe {
        libc::select(
            read_fd + 1,
            &mut fds,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut tv,
        )
    };

    if ready <= 0 {
        unsafe { libc::close(read_fd) };
        unsafe { libc::kill(pid, libc::SIGKILL) };
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
        eprintln!("[sim] Driver probe timed out — assuming default driver is OK");
        return false;
    }

    let mut byte: u8 = 0;
    unsafe {
        libc::read(read_fd, &mut byte as *mut u8 as *mut libc::c_void, 1);
    }
    unsafe { libc::close(read_fd) };

    let mut status: libc::c_int = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };

    if libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == 11 {
        eprintln!("[sim] Default driver crashed (SIGSEGV) — switching to software rendering");
        eprintln!("[sim] Set SDL_VIDEODRIVER to override. See https://github.com/Amperstrand/micronuts/issues/4");
        return true;
    }

    if byte == 1 {
        eprintln!("[sim] Default driver OK");
    } else {
        eprintln!("[sim] Default driver failed — switching to software rendering");
        return true;
    }

    false
}

fn apply_sdl_video_workaround() {
    if sdl2_crashes_with_default_driver() {
        std::env::set_var("SDL_VIDEODRIVER", "software");
    }
}

fn main() {
    apply_sdl_video_workaround();

    println!("Micronuts Native Simulator (embassy)");
    println!("=====================================");
    println!("Display: {}x{} RGB565 (portrait)", WIDTH, HEIGHT);
    println!("Transport: stdin/stdout (binary protocol)");
    println!("Click window to simulate touch input");
    println!("Press ESC or close window to exit");
    println!();

    let hw = MockHardware::new();
    let mut executor = embassy_executor::Executor::new();
    let executor: &'static mut embassy_executor::Executor =
        unsafe { core::mem::transmute(&mut executor) };
    executor.run(|spawner: embassy_executor::Spawner| {
        spawner.spawn(run(hw).expect("spawn failed"));
    });
}

#[embassy_executor::task]
async fn run(mut hw: MockHardware) {
    micronuts_app::run(&mut hw).await
}
