//! Browser pointer callbacks83620/83848/83A58/83CD0/83EEC and inertia80AA0.
use super::Row;
const EPSILON: f32 = f32::from_bits(0x34000000);
#[derive(Clone, Copy, Debug, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    List,
    None,
    Close,
}
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub captured: bool,
    pub area: Area,
    pub row: Option<usize>,
}
pub struct Context<'a> {
    pub rows: &'a [Row],
    pub selected: usize,
    pub scroll: f32,
    pub minimum: f32,
    pub bottom: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Native reveal uses4 ticks/easing0; immediate requests replace the tween.
    Select {
        row: usize,
        scroll: f32,
        immediate: bool,
    },
    Close,
    ReplayVoice {
        row: usize,
    },
    QuickLoad {
        row: usize,
    },
    ChapterSelector,
    /// Native target values use a0.2tick easing64 tween after clearing callbacks.
    CloseFlash {
        value: f32,
    },
    VoiceFlash {
        row: usize,
        value: f32,
    },
}
pub struct Pointer {
    area: Area,
    row: Option<usize>,
    movement: f32,
}
impl Default for Pointer {
    fn default() -> Self {
        Self {
            area: Area::List,
            row: None,
            movement: 0.,
        }
    }
}
impl Pointer {
    pub fn area(&self) -> Area {
        self.area
    }
    pub fn row(&self) -> Option<usize> {
        self.row
    }
    pub fn movement(&self) -> f32 {
        self.movement
    }
    ///83830 invalidates contact targets. The host also cancels its inherited
    /// contacts, preventing a later release from reaching this inactive owner.
    pub fn cancel(&mut self) {
        self.area = Area::List;
        self.row = None;
    }
    pub fn stop_movement(&mut self) {
        self.movement = 0.;
    }
    pub fn hover(&mut self, point: Point, ctx: &Context<'_>) -> Hit {
        self.row = None;
        if (46.0..154.0).contains(&point.y) && (1766.0..1874.0).contains(&point.x) {
            self.area = Area::Close;
            return Hit {
                captured: true,
                area: self.area,
                row: None,
            };
        }
        self.area = if (160.0..992.0).contains(&point.y) {
            Area::List
        } else {
            Area::None
        };
        if self.area == Area::List {
            if !(160.0..1760.0).contains(&point.x) {
                return Hit {
                    captured: false,
                    area: self.area,
                    row: None,
                };
            }
            let position = ((ctx.scroll as i32).saturating_sub(160) as f32) + point.y;
            let mut i = ctx.selected as isize;
            while let Some(row) = usize::try_from(i).ok().and_then(|i| ctx.rows.get(i)) {
                let bottom = row.top + row.height;
                if position < row.top {
                    if row.top - position < 8. {
                        break;
                    }
                    i -= 1;
                } else if position <= bottom {
                    if row.top + 18. <= position && position < bottom - 18. {
                        self.row = Some(i as usize);
                    }
                    break;
                } else {
                    if position - bottom < 8. {
                        break;
                    }
                    i += 1;
                }
            }
            if self.movement.abs() >= EPSILON {
                self.row = None;
                self.movement = 0.;
            }
        }
        Hit {
            captured: true,
            area: self.area,
            row: self.row,
        }
    }
    fn select(ctx: &Context<'_>, row: usize) -> Event {
        let r = ctx.rows[row];
        let relative = r.top - ctx.scroll;
        let scroll = if relative < 0. {
            r.top.max(ctx.minimum)
        } else if relative + r.height > 832. {
            (r.top + r.height - 832.)
                .min(ctx.bottom - 832.)
                .max(ctx.minimum)
        } else {
            ctx.scroll
        };
        Event::Select {
            row,
            scroll,
            immediate: false,
        }
    }
    fn over_row(point: Point, ctx: &Context<'_>, row: usize) -> bool {
        let y = (ctx.scroll as i32).saturating_sub(160) as f32 + point.y;
        let r = ctx.rows[row];
        y >= r.top && y < r.top + r.height && (160.0..1760.0).contains(&point.x)
    }
    pub fn press(&mut self, point: Point, ctx: &Context<'_>) -> Vec<Event> {
        if self.area == Area::Close {
            return vec![Event::CloseFlash { value: 0.7 }];
        }
        let Some(row) = self.row.filter(|&i| i < ctx.rows.len()) else {
            return vec![];
        };
        let mut events = vec![Self::select(ctx, row)];
        if Self::over_row(point, ctx, row)
            && point.x - 160. < 236.
            && ctx.rows[row].kind == 0
            && ctx.rows[row].voice_count != 0
        {
            events.push(Event::VoiceFlash { row, value: 0.7 });
        }
        events
    }
    pub fn release(&mut self, point: Point, ctx: &Context<'_>) -> Vec<Event> {
        if self.area == Area::Close {
            return vec![Event::Close];
        }
        let Some(row) = self.row.take().filter(|&i| i < ctx.rows.len()) else {
            return vec![];
        };
        let mut events = vec![Self::select(ctx, row)];
        if Self::over_row(point, ctx, row) {
            if point.x - 160. < 236. {
                if ctx.rows[row].kind == 0 && ctx.rows[row].voice_count != 0 {
                    events.push(Event::ReplayVoice { row });
                }
            } else {
                events.push(Event::QuickLoad { row });
            }
        }
        events
    }
    fn angle(x: f32, y: f32) -> f32 {
        (y.atan2(x) / f32::from_bits(0x40490fd8)) * 180.
    }
    fn visible(ctx: &Context<'_>, scroll: f32) -> usize {
        let mut selected = ctx.selected;
        let mut direction = 0;
        loop {
            let row = ctx.rows[selected];
            if row.top < scroll {
                if direction == -1 || selected + 1 >= ctx.rows.len() {
                    break;
                }
                direction = 1;
                selected += 1;
            } else if row.top + row.height > scroll + 832. {
                if direction == 1 || selected == 0 {
                    break;
                }
                direction = -1;
                selected -= 1;
            } else {
                break;
            }
        }
        selected
    }
    pub fn drag(&mut self, from: Point, to: Point, ctx: &Context<'_>) -> Vec<Event> {
        if self.area != Area::List {
            self.area = Area::None;
            return vec![Event::CloseFlash { value: 0. }];
        }
        let mut events = self
            .row
            .take()
            .map(|row| Event::VoiceFlash { row, value: 0. })
            .into_iter()
            .collect::<Vec<_>>();
        let dy = to.y - from.y;
        let angle = Self::angle(to.x - from.x, dy);
        if !(-150.0..=150.0).contains(&angle) || (-30.0..30.0).contains(&angle) && angle != -30. {
            return events;
        }
        let scroll = (ctx.scroll - dy).clamp(ctx.minimum, (ctx.bottom - 832.).max(ctx.minimum));
        events.push(Event::Select {
            row: Self::visible(ctx, scroll),
            scroll,
            immediate: true,
        });
        events
    }
    pub fn fling(&mut self, velocity: Point) -> Vec<Event> {
        if self.area != Area::List {
            return vec![];
        }
        let angle = Self::angle(velocity.x, velocity.y);
        let norm = velocity
            .x
            .mul_add(velocity.x, velocity.y * velocity.y)
            .sqrt();
        if norm > 20. && !(-150.0..=150.0).contains(&angle) {
            return vec![Event::ChapterSelector];
        }
        self.movement = velocity.y;
        vec![]
    }
    ///80AA0 runs after private children and before the fixed1tick scroll tween.
    pub fn advance(&mut self, delta: f32, ctx: &Context<'_>) -> Vec<Event> {
        if self.movement.abs() < EPSILON {
            return vec![];
        }
        let scroll = (-self.movement)
            .mul_add(delta, ctx.scroll)
            .clamp(ctx.minimum, (ctx.bottom - 832.).max(ctx.minimum));
        if (ctx.scroll - scroll).abs() < EPSILON {
            self.movement = 0.;
            return vec![];
        }
        let row = Self::visible(ctx, scroll);
        self.movement *= f32::from_bits(0x3f733333).powf(delta);
        if self.movement.abs() < 0.1 {
            self.movement = 0.;
        }
        vec![Event::Select {
            row,
            scroll,
            immediate: true,
        }]
    }
}
