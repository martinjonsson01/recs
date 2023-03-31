use std::f32::consts::FRAC_PI_2;
use std::time::Duration;

use cgmath::{perspective, InnerSpace, Matrix4, Point3, Rad, Vector3};
use winit::dpi::PhysicalPosition;

use crate::window::{ElementState, InputEvent, MouseButton, MouseScrollDelta, VirtualKeyCode};
use crate::EventPropagation;

/// The coordinate system in wgpu is based on DirectX's and Metal's coordinate systems.
/// This means that in normalized device coordinates, the x- and y-axis span [-1, 1], with
/// z spanning [0, 1]. cgmath is built for OpenGL's coordinate system, so we need to transform it.
#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: Matrix4<f32> = Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

const SAFE_FRAC_PI_2: f32 = FRAC_PI_2 - 0.0001;

/// The eye from which a scene is rendered.
#[derive(Debug)]
pub struct Camera {
    pub position: Point3<f32>,
    /// The horizontal rotation from side to side.
    yaw: Rad<f32>,
    /// The vertical rotation upwards and downwards.
    pitch: Rad<f32>,
}

impl Camera {
    pub fn new<Pos: Into<Point3<f32>>, Yaw: Into<Rad<f32>>, Pitch: Into<Rad<f32>>>(
        position: Pos,
        yaw: Yaw,
        pitch: Pitch,
    ) -> Self {
        Self {
            position: position.into(),
            yaw: yaw.into(),
            pitch: pitch.into(),
        }
    }

    /// Constructs the view matrix for the camera, centering the camera at the origin
    /// with the z-axis pointing outwards from the camera.
    pub fn view_matrix(&self) -> Matrix4<f32> {
        let (sin_pitch, cos_pitch) = self.pitch.0.sin_cos();
        let (sin_yaw, cos_yaw) = self.yaw.0.sin_cos();

        Matrix4::look_to_rh(
            self.position,
            Vector3::new(cos_pitch * cos_yaw, sin_pitch, cos_pitch * sin_yaw).normalize(),
            Vector3::unit_y(),
        )
    }
}

/// The perspective projection.
#[derive(Debug)]
pub struct Projection {
    /// The aspect ratio of the viewport.
    aspect: f32,
    /// The vertical field of view.
    fovy: Rad<f32>,
    /// The near clipping plane.
    znear: f32,
    /// The far clipping plane.
    zfar: f32,
}

impl Projection {
    pub fn new<Fov: Into<Rad<f32>>>(
        width: u32,
        height: u32,
        fovy: Fov,
        znear: f32,
        zfar: f32,
    ) -> Self {
        Self {
            aspect: width as f32 / height as f32,
            fovy: fovy.into(),
            znear,
            zfar,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.aspect = width as f32 / height as f32;
    }

    /// Constructs the perspective projection matrix that transforms from view space to clip space.
    ///
    /// (View-coordinates -> Normalized Device Coordinates)
    ///
    /// This is a right-handed projection matrix, with the z-axis pointing out of the screen.
    pub fn perspective_matrix(&self) -> Matrix4<f32> {
        // See documentation for OPENGL_TO_WGPU_MATRIX.
        OPENGL_TO_WGPU_MATRIX * perspective(self.fovy, self.aspect, self.znear, self.zfar)
    }
}

/// An FPS-style controller for moving around and rotating the camera.
#[derive(Debug)]
pub struct CameraController {
    amount_left: f32,
    amount_right: f32,
    amount_forward: f32,
    amount_backward: f32,
    amount_up: f32,
    amount_down: f32,
    rotate_horizontal: f32,
    rotate_vertical: f32,
    scroll: f32,
    speed: f32,
    sensitivity: f32,
    mouse_pressed: bool,
}

impl CameraController {
    pub(crate) fn new(speed: f32, sensitivity: f32) -> Self {
        Self {
            amount_left: 0.0,
            amount_right: 0.0,
            amount_forward: 0.0,
            amount_backward: 0.0,
            amount_up: 0.0,
            amount_down: 0.0,
            rotate_horizontal: 0.0,
            rotate_vertical: 0.0,
            scroll: 0.0,
            speed,
            sensitivity,
            mouse_pressed: false,
        }
    }

    pub(crate) fn input(&mut self, event: &InputEvent) -> EventPropagation {
        match event {
            InputEvent::Keyboard { key, state } => self.process_keyboard(*key, *state),
            InputEvent::Scroll(delta) => {
                self.process_scroll(delta);
                EventPropagation::Consume
            }
            InputEvent::MouseClick {
                button: MouseButton::Left,
                state,
                ..
            } => {
                self.mouse_pressed = *state == ElementState::Pressed;
                EventPropagation::Consume
            }
            InputEvent::MouseMove {
                mouse_delta_x,
                mouse_delta_y,
            } => {
                if self.mouse_pressed {
                    self.process_mouse(*mouse_delta_x, *mouse_delta_y);
                    EventPropagation::Consume
                } else {
                    EventPropagation::Propagate
                }
            }
            _ => EventPropagation::Propagate,
        }
    }

    fn process_keyboard(&mut self, key: VirtualKeyCode, state: ElementState) -> EventPropagation {
        let amount = if state == ElementState::Pressed {
            1.0
        } else {
            0.0
        };
        match key {
            VirtualKeyCode::W | VirtualKeyCode::Up => {
                self.amount_forward = amount;
                EventPropagation::Consume
            }
            VirtualKeyCode::S | VirtualKeyCode::Down => {
                self.amount_backward = amount;
                EventPropagation::Consume
            }
            VirtualKeyCode::A | VirtualKeyCode::Left => {
                self.amount_left = amount;
                EventPropagation::Consume
            }
            VirtualKeyCode::D | VirtualKeyCode::Right => {
                self.amount_right = amount;
                EventPropagation::Consume
            }
            VirtualKeyCode::Space => {
                self.amount_up = amount;
                EventPropagation::Consume
            }
            VirtualKeyCode::LShift => {
                self.amount_down = amount;
                EventPropagation::Consume
            }
            _ => EventPropagation::Propagate,
        }
    }

    fn process_mouse(&mut self, mouse_dx: f64, mouse_dy: f64) {
        self.rotate_horizontal = mouse_dx as f32;
        self.rotate_vertical = mouse_dy as f32;
    }

    fn process_scroll(&mut self, delta: &MouseScrollDelta) {
        const LINE_HEIGHT_PX: f32 = 100.0;
        self.scroll = -match delta {
            MouseScrollDelta::LineDelta(_, scroll) => scroll * LINE_HEIGHT_PX,
            MouseScrollDelta::PixelDelta(PhysicalPosition { y: scroll, .. }) => *scroll as f32,
        };
    }

    /// Transforms the camera according to the previously recorded input.
    pub fn update_camera(&mut self, camera: &mut Camera, dt: Duration) {
        let dt = dt.as_secs_f32();

        // Move forward/backward and left/right
        let (yaw_sin, yaw_cos) = camera.yaw.0.sin_cos();
        let forward = Vector3::new(yaw_cos, 0.0, yaw_sin).normalize();
        let right = Vector3::new(-yaw_sin, 0.0, yaw_cos).normalize();
        camera.position += forward * (self.amount_forward - self.amount_backward) * self.speed * dt;
        camera.position += right * (self.amount_right - self.amount_left) * self.speed * dt;

        // Move in/out (aka. "zoom")
        // Note: this isn't an actual zoom. The camera's position changes when zooming.
        // This makes it easier to get closer to an object you want to focus on.
        let (pitch_sin, pitch_cos) = camera.pitch.0.sin_cos();
        let scroll_forward =
            Vector3::new(pitch_cos * yaw_cos, pitch_sin, pitch_cos * yaw_sin).normalize();
        camera.position += scroll_forward * self.scroll * self.speed * self.sensitivity * dt;
        self.scroll = 0.0;

        // Move up/down. Since we don't use roll, we can just modify the y coordinate directly.
        camera.position.y += (self.amount_up - self.amount_down) * self.speed * dt;

        // Rotate
        camera.yaw += Rad(self.rotate_horizontal) * self.sensitivity * dt;
        camera.pitch += Rad(-self.rotate_vertical) * self.sensitivity * dt;

        // If process_mouse isn't called every frame, these values will not get set to zero,
        // and the camera will rotate when moving in a non cardinal direction.
        self.rotate_horizontal = 0.0;
        self.rotate_vertical = 0.0;

        // Keep the camera's angle from going too high/low.
        if camera.pitch < -Rad(SAFE_FRAC_PI_2) {
            camera.pitch = -Rad(SAFE_FRAC_PI_2);
        } else if camera.pitch > Rad(SAFE_FRAC_PI_2) {
            camera.pitch = Rad(SAFE_FRAC_PI_2);
        }
    }
}
