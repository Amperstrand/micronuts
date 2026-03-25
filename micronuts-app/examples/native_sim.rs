use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::Pixel;
use rand::RngCore;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::Sdl;

use micronuts_app::display::{self, HEIGHT, WIDTH};
use micronuts_app::hardware::{MicronutsHardware, TouchPoint};
use micronuts_app::protocol::{Frame, FrameDecoder, Response, MAX_PAYLOAD_SIZE};
use micronuts_app::qr::{ScannerModel, ScannerState};
use micronuts_app::state::ScannerInfo;

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
    stdin_buf: Vec<u8>,
    pending_touch: Option<TouchPoint>,
}

impl MockHardware {
    fn new() -> Self {
        Self {
            display: Sdl2Display::new(),
            rng: rand::thread_rng(),
            decoder: FrameDecoder::new(),
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

    fn scanner_trigger(&mut self) -> Result<(), ()> {
        println!("[SCANNER] Trigger scan (simulated)");
        Ok(())
    }

    fn scanner_try_read(&mut self) -> Option<Vec<u8>> {
        None
    }

    fn scanner_status(&self) -> ScannerInfo {
        ScannerInfo {
            model: ScannerModel::Unknown,
            state: ScannerState::Uninitialized,
            last_scan_len: None,
            connected: false,
        }
    }

    fn transport_poll(&mut self) -> Option<Frame> {
        let mut buf = [0u8; 64];
        match io::stdin().read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => {
                self.stdin_buf.extend_from_slice(&buf[..n]);
            }
            Err(_) => return None,
        }

        let frame = self.decoder.decode(&self.stdin_buf);
        if frame.is_some() {
            self.stdin_buf.clear();
        }
        frame
    }

    fn transport_send(&mut self, response: &Response) {
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
                return Some(tp);
            }
        }
        None
    }

    fn delay_ms(&mut self, ms: u32) {
        self.display.present();
        thread::sleep(Duration::from_millis(ms as u64));
    }
}

fn main() {
    println!("Micronuts Native Simulator");
    println!("==========================");
    println!("Display: {}x{} RGB565", WIDTH, HEIGHT);
    println!("Transport: stdin/stdout (binary protocol)");
    println!("Click window to simulate touch input");
    println!("Press ESC or close window to exit");
    println!();

    let mut hw = MockHardware::new();
    micronuts_app::run(&mut hw);
}
