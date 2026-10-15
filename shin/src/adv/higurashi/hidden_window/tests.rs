use super::*;
use crate::adv::{input_area::{PointerEvent, PointerKind}, higurashi::mode_stack::{Mode, ModePush, ModeStack}};
use anyhow::Result;
use shin_core::time::Ticks;
use std::{fmt::Write, io::Write as IoWrite, process::{Command, Stdio}};

#[derive(Default)]
struct Owners {
    message: u8,
    indicator: u8,
    menu: u8,
    overlay: bool,
    edges: Vec<[u32; 4]>,
}
impl ConcealServices for Owners {
    fn hide_message(&mut self, hidden: bool) { self.message = u8::from(hidden); }
    fn hide_overlay(&mut self, hidden: bool) {
        if self.overlay { self.edges.push([u32::from(hidden), self.message.into(), self.indicator.into(), self.menu.into()]); }
    }
    fn hide_indicator(&mut self, hidden: bool) { self.indicator = u8::from(hidden); }
    fn hide_menu(&mut self, hidden: bool) { self.menu = u8::from(hidden); }
}
#[derive(Default)]
struct Context {
    owners: Owners,
    conceal: ConcealState,
    number: u32,
    ticks: u64,
    pressed: u32,
    events: Vec<PointerEvent>,
}
struct Window(HiddenWindow);
impl Mode<Context> for Window {
    fn completion(&mut self) -> u32 { self.0.completion_word() }
    fn poll(&mut self, _: Ticks, permission: bool, services: &mut Context, _: &mut ModePush<'_, Self>) -> Result<u32> {
        Ok(self.0.poll(u32::from(permission), services.pressed, &PointerFrame {
            number: services.number, time_seconds: services.ticks as f64 * 1e-6, events: &services.events,
        }))
    }
    fn retire(&mut self, services: &mut Context) { services.conceal.control(0, &mut services.owners); }
}
#[derive(Default)]
struct Comparison {
    stack: ModeStack<Window>,
    context: Context,
    input: String,
    expected: Vec<String>,
}
impl Comparison {
    fn row(&mut self, op: char, permission: bool) {
        let owners = &mut self.context.owners;
        let complete = self.stack.iter().next().map_or(256, |mode| mode.0.completion_word());
        let mut row = format!("O {op} {} {} {} {} {} {} {complete} {}", u32::from(permission), self.context.conceal.depth, owners.message, owners.indicator, owners.menu, self.stack.len(), owners.edges.len());
        for edge in owners.edges.drain(..) { for word in edge { write!(row, " {word}").unwrap(); } }
        row.push_str(" 1");self.expected.push(row);
    }
    fn begin(&mut self, depth: u32, flags: u32, done: u8, overlay: bool) {
        assert!(self.stack.is_empty());
        let [message, indicator, menu] = [flags & 1, flags >> 1 & 1, flags >> 2 & 1].map(|n| n as u8);
        writeln!(self.input,"N {depth} {message} {indicator} {menu} {done} {}",u32::from(overlay)).unwrap();
        self.context = Context { owners: Owners { message, indicator, menu, overlay, ..Default::default() }, conceal: ConcealState { depth }, ..Default::default() };
        let mut window = HiddenWindow::default();
        self.context.conceal.control(1, &mut self.context.owners);
        window.completed = done;self.stack.push(Window(window));self.row('N',false);
    }
    fn frame(&mut self, number: u32, ticks: u64, permission: u32, pressed: u32, events: &[PointerEvent]) {
        writeln!(self.input,"F {number} {ticks} {permission} {pressed:x} {}",events.len()).unwrap();
        for event in events { writeln!(self.input,"{} {} {} {}",event.kind as i32,event.id,event.position.x,event.position.y).unwrap(); }
        self.context.number=number;self.context.ticks=ticks;self.context.pressed=pressed;self.context.events=events.to_vec();
        let permission = self.stack.poll(Ticks::from_f32(1.0),permission,&mut self.context).unwrap();
        self.stack.draw(|_| {});self.row('F',permission);
    }
    fn control(&mut self, raw: u32) {
        writeln!(self.input,"H {raw}").unwrap();self.context.conceal.control(raw,&mut self.context.owners);self.row('H',false);
    }
    fn destroy(&mut self) {
        self.input.push_str("D\n");self.stack.clear_with(&mut self.context);self.row('D',false);
    }
}
fn event(kind: PointerKind,id: i32,x: i32,y: i32) -> PointerEvent { PointerEvent { kind,id,position:glam::ivec2(x,y) } }

#[test]
#[ignore = "executes complete updated AArch64 HiddenWindow/concealment/contact/mode-stack functions"]
fn higurashi_hidden_window_contacts_counter_and_retirement_match_native() {
    use PointerKind::*;
    let helper=std::env::var("SHIN_HIGURASHI_NATIVE_HIDDEN_WINDOW").unwrap();
    let exe=std::env::var("SHIN_HIGURASHI_EXE").unwrap();
    let mut comparison=Comparison::default();let mut timelines=0;
    for depth in [0,1,u32::MAX] {
        for overlay in [false,true] {
            for done in [0,1,2,3,254,255] {
                for permission in [0,1,2,3,u32::MAX-1,u32::MAX] {
                    let keys=(0..32).map(|bit|1u32<<bit).chain([0,0x0300_9000,0x0100_0040,u32::MAX,0x400,0x800]);
                    for (index,pressed) in keys.enumerate() {
                        comparison.begin(depth,index as u32%8,done,overlay);
                        comparison.frame(1,0,permission,pressed,&[]);
                        comparison.frame(2,150000,permission,pressed,&[]);
                        comparison.destroy();timelines+=1;
                    }
                }
            }
        }
    }
    for depth in [0,1,2,u32::MAX-1,u32::MAX] {
        for flags in 0..8 {
            for overlay in [false,true] {
                comparison.begin(depth,flags,0,overlay);
                for raw in [1,3,0,2,u32::MAX,u32::MAX-1] { comparison.control(raw); }
                comparison.destroy();timelines+=1;
            }
        }
    }
    for pattern in 0..15 {
        for permission in [0,1,2,3,u32::MAX-1,u32::MAX] {
            for depth in [0,1,u32::MAX] {
                for done in [0,2] {
                    for overlay in [false,true] {
                        comparison.begin(depth,pattern%8,done,overlay);
                        let mut frames: [Vec<PointerEvent>;8]=std::array::from_fn(|_|Vec::new());
                        frames[0].push(event(Down,4,100,100));
                        match pattern {
                            0 => frames[1].push(event(Up,4,900,900)),
                            1 => frames[5].push(event(Up,4,0,0)),
                            2 => {frames[1].push(event(Move,4,120,100));frames[2].push(event(Up,4,0,0));}
                            3 => frames[1].push(event(Cancel,4,0,0)),
                            4 => {frames[1].push(event(Down,8,200,100));frames[2].push(event(Up,4,0,0));frames[3].push(event(Up,8,0,0));}
                            5 => {frames[1].push(event(Up,99,0,0));frames[2].push(event(Up,4,0,0));}
                            6 => {frames[0].clear();frames[1].push(event(Move,99,0,0));}
                            7 => {frames[1].push(event(Up,4,0,0));frames[3].push(event(Up,4,0,0));frames[4].push(event(Up,4,0,0));}
                            8 => {frames[2].push(event(Move,4,105,100));frames[3].push(event(Up,4,0,0));}
                            9 => frames[0].push(event(Up,4,0,0)),
                            10 => {frames[1].push(event(Move,4,110,100));frames[2].push(event(Up,4,0,0));}
                            11 => {frames[1].push(event(Move,4,111,100));frames[2].push(event(Up,4,0,0));}
                            12 => {frames[1].extend([event(Down,8,200,100),event(Move,4,300,100)]);frames[2].extend([event(Up,8,0,0),event(Up,4,0,0)]);}
                            13 => {frames[0].clear();frames[1].push(event(Up,4,0,0));}
                            _ => {frames[0].clear();}
                        }
                        for (step,events) in frames.iter().enumerate() {
                            let number=if pattern==7 {[0,0,1,1,2,3,4,5][step]}else{step as u32};
                            let permission=if pattern==8 && step==2 {0}else{permission};
                            let pressed=if step==6 {1<<25}else if pattern==14 && step<4 {1<<6}else if pattern==14 {1<<12}else{0};
                            comparison.frame(number,[0,149999,150000,150001,1150000,1300000,1400000,1500000][step],permission,pressed,events);
                        }
                        comparison.destroy();timelines+=1;
                    }
                }
            }
        }
    }
    let mut child=Command::new(helper).arg(exe).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin=child.stdin.take().unwrap();let input=comparison.input;
    let writer=std::thread::spawn(move||stdin.write_all(input.as_bytes()).unwrap());
    let output=child.wait_with_output().unwrap();writer.join().unwrap();
    assert!(output.status.success(),"native fixture failed:{:?} {}",output.status,String::from_utf8_lossy(&output.stderr));
    let output=String::from_utf8(output.stdout).unwrap();let rows:Vec<_>=output.lines().collect();assert_eq!(rows.len(),comparison.expected.len());
    for (index,(actual,expected)) in rows.iter().zip(&comparison.expected).enumerate(){assert_eq!(actual,expected,"HiddenWindow observation{index}");}
    assert_eq!(timelines,9368);
    println!("Higurashi HiddenWindow:{timelines} native timelines,{} exact contact/counter/visibility/retirement observations pass",rows.len());
}

mod host;
