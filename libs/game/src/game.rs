use features::{log, GLOBAL_ERROR_LOGGER, GLOBAL_LOGGER};
use platform_types::{Button, Input, Speaker, State, StateParams, SFX};
use rendering::{Framebuffer, BLUE, GREEN, PALETTE, PURPLE, RED, WHITE, YELLOW};

const GRID_WIDTH: u8 = 40;
const GRID_HEIGHT: u8 = 60;
const GRID_LENGTH: usize = GRID_WIDTH as usize * GRID_HEIGHT as usize;

type Grid = [Option<HalfHexSpec>; GRID_LENGTH];

macro_rules! on_left {
    ($x: expr) => {
        $x & 1 == 0
    };
    ($x: expr, bit) => {
        $x & 1
    };
}

type HalfHexSpec = u8;

fn get_colours(mut spec: HalfHexSpec) -> (u32, u32) {
    spec &= 0b0111_0111;
    (
        PALETTE[(spec & 0b111) as usize],
        PALETTE[(spec >> 4) as usize],
    )
}

#[derive(Clone, Copy)]
enum Cursor {
    Unselected(usize),
    Selected(usize, usize),
}

impl Cursor {
    fn wrapping_add(self, other: usize) -> Cursor {
        use Cursor::*;
        match self {
            Unselected(c) => Unselected(c.wrapping_add(other)),
            Selected(c1, c2) => Selected(c1, c2.wrapping_add(other)),
        }
    }
}

use std::convert::From;

impl From<Cursor> for usize {
    fn from(c: Cursor) -> Self {
        use Cursor::*;
        match c {
            Unselected(c) => c,
            Selected(_, c2) => c2,
        }
    }
}

impl Cursor {
    fn iter(&self) -> impl Iterator<Item = usize> {
        use Cursor::*;
        match *self {
            Unselected(c) => vec![c].into_iter(),
            Selected(c1, c2) => vec![c1, c2].into_iter(),
        }
    }
}

struct Animation {
    x: u8,
    y: u8,
    target_x: u8,
    target_y: u8,
    x_rate: u8,
    y_rate: u8,
    spec: HalfHexSpec,
}

use std::cmp::{max, min};

const DELAY_FACTOR: u8 = 16;

impl Animation {
    pub fn new(i: usize, target_i: usize, spec: HalfHexSpec) -> Self {
        let (x, y) = i_to_xy(i);
        let (target_x, target_y) = i_to_xy(target_i);

        let (x_diff, y_diff) = (
            if target_x == x {
                0
            } else if x > target_x {
                x - target_x
            } else {
                target_x - x
            },
            if target_y == y {
                0
            } else if y > target_y {
                y - target_y
            } else {
                target_y - y
            },
        );

        Animation {
            x,
            y,
            x_rate: max(x_diff / DELAY_FACTOR, 1),
            y_rate: max(y_diff / DELAY_FACTOR, 1),
            target_x,
            target_y,
            spec,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.x == self.target_x && self.y == self.target_y
    }

    pub fn approach_target(&mut self) {
        let (d_x, d_y) = self.get_delta();

        self.x = match d_x {
            x if x > 0 => self.x.saturating_add(x as u8),
            x if x < 0 => self.x.saturating_sub(x.abs() as u8),
            _ => self.x,
        };
        self.y = match d_y {
            y if y > 0 => self.y.saturating_add(y as u8),
            y if y < 0 => self.y.saturating_sub(y.abs() as u8),
            _ => self.y,
        };
    }

    fn get_delta(&self) -> (i8, i8) {
        (
            if self.target_x == self.x {
                0
            } else if self.x > self.target_x {
                let x_diff = self.x - self.target_x;
                -(min(x_diff, self.x_rate) as i8)
            } else {
                let x_diff = self.target_x - self.x;
                min(x_diff, self.x_rate) as i8
            },
            if self.target_y == self.y {
                0
            } else if self.y > self.target_y {
                let y_diff = self.y - self.target_y;
                -(min(y_diff, self.y_rate) as i8)
            } else {
                let y_diff = self.target_y - self.y;
                min(y_diff, self.y_rate) as i8
            },
        )
    }
}

pub struct GameState {
    grid: Grid,
    cursor: Cursor,
    frame_counter: usize,
    animations: Vec<Animation>,
}

fn new_grid() -> Grid {
    let mut grid: Grid = [None; GRID_LENGTH];
    let mut c: HalfHexSpec = 0;
    for i in 0..GRID_LENGTH {
        grid[i] = Some(c);
        c = c.wrapping_add(1);
    }
    grid
}

impl GameState {
    pub fn new(_seed: [u8; 16]) -> GameState {
        let grid: Grid = new_grid();

        GameState {
            grid,
            cursor: Cursor::Unselected(GRID_WIDTH as usize + 1),
            frame_counter: 0,
            animations: Vec::with_capacity(8),
        }
    }
}

pub struct EntireState {
    pub game_state: GameState,
    pub framebuffer: Framebuffer,
    pub input: Input,
    pub speaker: Speaker,
}

impl EntireState {
    pub fn new((seed, logger, error_logger): StateParams) -> Self {
        let framebuffer = Framebuffer::new();

        unsafe {
            GLOBAL_LOGGER = logger;
            GLOBAL_ERROR_LOGGER = error_logger;
        }

        EntireState {
            game_state: GameState::new(seed),
            framebuffer,
            input: Input::new(),
            speaker: Speaker::new(),
        }
    }
}

impl State for EntireState {
    fn frame(&mut self, handle_sound: fn(SFX)) {
        update_and_render(
            &mut self.framebuffer,
            &mut self.game_state,
            self.input,
            &mut self.speaker,
        );

        self.input.previous_gamepad = self.input.gamepad;

        for request in self.speaker.drain() {
            handle_sound(request);
        }
    }

    fn press(&mut self, button: Button::Ty) {
        if self.input.previous_gamepad.contains(button) {
            //This is meant to pass along the key repeat, if any.
            //Not sure if rewriting history is the best way to do this.
            self.input.previous_gamepad.remove(button);
        }

        self.input.gamepad.insert(button);
    }

    fn release(&mut self, button: Button::Ty) {
        self.input.gamepad.remove(button);
    }

    fn get_frame_buffer(&self) -> &[u32] {
        &self.framebuffer.buffer
    }
}

const HEX_WIDTH: u8 = 4;
const HEX_HEIGHT: u8 = 8;
const HALF_HEX_HEIGHT: u8 = HEX_HEIGHT / 2;
const EDGE_OFFSET: u8 = 6;

const ROW_TYPES: u8 = 3;

fn p_xy(x: u8, y: u8) -> (u8, u8) {
    let x_offset = (y % ROW_TYPES) * HEX_WIDTH;
    if on_left!(x) {
        (
            x * 6 + x_offset + EDGE_OFFSET,
            y * HALF_HEX_HEIGHT + EDGE_OFFSET,
        )
    } else {
        (
            x * 6 + x_offset - 2 + EDGE_OFFSET,
            y * HALF_HEX_HEIGHT + EDGE_OFFSET,
        )
    }
}

//This way we don't need to allocate a closure every frame.
fn marching_ants(frame_counter: usize) -> fn(usize, usize, usize, usize) -> u32 {
    macro_rules! marching_ants {
        ($offset: expr) => {{
            fn _marching_ants(x: usize, y: usize, _: usize, _: usize) -> u32 {
                if (x + y + $offset) & 2 == 0 {
                    YELLOW
                } else {
                    PURPLE
                }
            }

            _marching_ants
        }};
    }

    match frame_counter & 0b1_1000 {
        0 => marching_ants!(0),
        0b0_1000 => marching_ants!(1),
        0b1_0000 => marching_ants!(2),
        _ => marching_ants!(3),
    }
}

//see `design/gridMovement.md` for the derivation of this table.
static MOVEMENT: [i8; 24] = {
    const W: i8 = GRID_WIDTH as i8;

    [
        -(W + 1),
        2 * W - 1,
        W - 1,
        1,
        -(2 * W + 1),
        W - 1,
        -1,
        -(W + 1),
        -(W - 1),
        2 * W + 1,
        W - 1,
        1,
        -(2 * W + 1),
        W - 1,
        -1,
        -(W - 1),
        -(W - 1),
        2 * W + 1,
        W + 1,
        1,
        -(2 * W - 1),
        W + 1,
        -1,
        -(W - 1),
    ]
};

enum Dir {
    Up,
    Down,
    Left,
    Right,
}

fn get_movement_offset(x: u8, y: u8, dir: Dir) -> i8 {
    let index = ((y % ROW_TYPES) << 3) | (on_left!(x, bit) << 2) | dir as u8;

    MOVEMENT[index as usize]
}

fn i_to_xy(i: usize) -> (u8, u8) {
    (
        (i % GRID_WIDTH as usize) as u8,
        (i / GRID_WIDTH as usize) as u8,
    )
}

fn xy_to_i(x: u8, y: u8) -> usize {
    y as usize * GRID_WIDTH as usize + x as usize
}

fn draw_hexagon(framebuffer: &mut Framebuffer, x: u8, y: u8, spec: HalfHexSpec) {
    let (inside, outline) = get_colours(spec);

    let (p_x, p_y) = p_xy(x, y);
    if on_left!(x) {
        framebuffer.hexagon_left(p_x, p_y, inside, outline);
    } else {
        framebuffer.hexagon_right(p_x, p_y, inside, outline);
    }
}

#[inline]
pub fn update_and_render(
    framebuffer: &mut Framebuffer,
    state: &mut GameState,
    input: Input,
    _speaker: &mut Speaker,
) {
    //
    //UPDATE
    //
    for animation_index in (0..state.animations.len()).rev() {
        let animation = &mut state.animations[animation_index];
        animation.approach_target();

        if animation.is_complete() {
            let index = xy_to_i(animation.x, animation.y);

            state.grid[index] = Some(animation.spec);

            let other_index = if on_left!(animation.x) {
                index + 1
            } else {
                index - 1
            };
            if state.grid[other_index].map(get_colours) == state.grid[index].map(get_colours) {
                state.grid[other_index] = None;
                state.grid[index] = None;
            }

            state.animations.swap_remove(animation_index);
        }
    }

    match input.gamepad {
        Button::B => framebuffer.clear_to(BLUE),
        Button::Select => framebuffer.clear_to(WHITE),
        Button::Start => framebuffer.clear_to(RED),
        _ => {}
    }

    if input.pressed_this_frame(Button::A) {
        match state.cursor {
            Cursor::Unselected(c) => {
                if state.grid[c].is_some() {
                    state.cursor = Cursor::Selected(c, c);
                }
            }
            Cursor::Selected(c1, c2) => {
                if let (Some(h1), Some(h2)) = (state.grid[c1], state.grid[c2]) {
                    state.grid[c1] = None;
                    state.grid[c2] = None;
                    state.animations.push(Animation::new(c1, c2, h1));
                    state.animations.push(Animation::new(c2, c1, h2));
                    state.cursor = Cursor::Unselected(c2);
                }
            }
        };
    }

    macro_rules! move_hex {
        ($dir: expr) => {
            let cursor_num: usize = state.cursor.into();

            let (x, y) = i_to_xy(cursor_num);

            let offset: i8 = get_movement_offset(x, y, $dir);

            let new_cursor = state.cursor.wrapping_add(offset as usize);
            let new_cursor_num: usize = new_cursor.into();

            if new_cursor_num < GRID_LENGTH {
                let width = GRID_WIDTH as usize;
                let new_x = new_cursor_num % width;
                let looped =
                    (x == 0 && new_x == width - 1) || (x as usize == width - 1 && new_x == 0);
                if !looped {
                    state.cursor = new_cursor;
                }
            }
        };
    }

    if input.pressed_this_frame(Button::Up) {
        move_hex!(Dir::Up);
    }
    if input.pressed_this_frame(Button::Down) {
        move_hex!(Dir::Down);
    }
    if input.pressed_this_frame(Button::Left) {
        move_hex!(Dir::Left);
    }
    if input.pressed_this_frame(Button::Right) {
        move_hex!(Dir::Right);
    }

    //
    // RENDER
    //

    framebuffer.clear_to(framebuffer.buffer[0]);

    for y in 0..GRID_HEIGHT {
        for x in 0..GRID_WIDTH {
            if let Some(spec) = state.grid[xy_to_i(x, y)] {
                draw_hexagon(framebuffer, x, y, spec);
            }
        }
    }

    for index in state.cursor.iter() {
        let (x, y) = i_to_xy(index);
        let (p_x, p_y) = p_xy(x, y);
        framebuffer.draw_rect_with_shader(
            p_x as usize - 1,
            p_y as usize - 1,
            6,
            10,
            marching_ants(state.frame_counter),
        );
    }

    for &Animation { x, y, spec, .. } in state.animations.iter() {
        draw_hexagon(framebuffer, x, y, spec);
    }

    state.frame_counter += 1;
}
