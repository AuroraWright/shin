//! SOURCE-ONLY Minagoroshi fixture-unlocked ordinary Runtime journey.
//! Intended child of journeys::sustained; not registered/compiled/admitted/run.
//! Requires proposed cfg(test) read-only Gameplay FakeSelect scalar getter.
use super::*;
use std::path::PathBuf;
use crate::adv::higurashi::voice_commands::VoiceCommands;
use shin_core::format::scenario::instruction_elements::Register;

const TEMPLATE_FNV:u64=0x83007166ab0dfcf0;
const FAKE_PC:u32=0xb0484e;
const CHOICE_PC:u32=0xa15692;
fn template()->(SaveData,Vec<u8>){
    let path=std::env::var("SHIN_HIGURASHI_CHAPTER_TEMPLATE").unwrap_or_else(|_|
        concat!(env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/higurashi/chapter-playthrough-20261015/unlocked-input.dat").into());
    let bytes=std::fs::read(path).unwrap();assert_eq!(bytes.len(),31748);
    let fnv=bytes.iter().fold(0xcbf29ce484222325u64,
        |h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3));
    assert_eq!(fnv,TEMPLATE_FNV);let saved=SaveData::decode(&bytes).unwrap();
    assert_eq!(saved.encode().unwrap(),bytes);(saved,bytes)
}
fn picker_ready(r:&Runtime)->bool{
    r.slot().and_then(|s|s.scenario_select.as_ref()).is_some_and(|p|{
        let o=p.reference_owner();let st=o.reference_state();let m=o.reference_model();
        st.0&&st.2&&st.3.is_none()&&m.ready&&!m.closing
    })
}
fn scenario(r:&Runtime)->Option<i32>{
    game(r)?.runtime_game_data().ok().map(|d|d.progress.active_entry().scenario_id)
}
fn pending(r:&Runtime,pc:u32)->bool{
    game(r).and_then(|g|g.host()).is_some_and(|h|
        h.runner.native_vm_attached()&&h.runner.position().0==pc&&h.runner.pending_command().is_some())
}
fn unchanged(j:&Journey,rows:&crate::app::higurashi::runtime_saves::Slots,
    settings:&[u8],ids:[usize;4]){
    let g=j.runtime.globals().unwrap().unwrap();
    assert_eq!(serde_json::to_vec(&g.data.settings).unwrap(),settings);
    assert_eq!([Arc::as_ptr(g.common)as usize,Arc::as_ptr(g.server)as usize,
        Arc::as_ptr(g.manager)as usize,Arc::as_ptr(g.sounds)as usize],ids);
    for(i,(a,b))in g.runtime_saves.unwrap().manual.iter().zip(&rows.manual).enumerate(){
        assert_eq!(a.valid,b.valid,"manual{i} validity");assert_eq!(a.data,b.data,"manual{i} fullpayload");
    }
}
fn capture(j:&mut Journey,root:&std::path::Path,label:&str){
    let state=game(&j.runtime).and_then(|g|g.reference_fake_select_state());
    let frame=j.runtime.frames;let ticks=j.runtime.globals().unwrap().unwrap().clock.ticks();
    for size in [(1280,720),(1920,1080)]{
        let a=j.gpu.pixels(&j.runtime,size,false);
        let b=j.gpu.pixels(&j.runtime,size,true);
        assert_eq!(a,b,"Completed native cache presentation/capture");
        assert!(a.chunks_exact(4).any(|p|p[..3]!=[0,0,0]),"Nonempty actual scene");
        std::fs::write(root.join(format!("{label}-{}x{}.rgba",size.0,size.1)),&a).unwrap();
        let hash=a.iter().fold(0xcbf29ce484222325u64,
            |h,b|(h^u64::from(*b)).wrapping_mul(0x100000001b3));
        eprintln!("MINAGO_CAPTURE {label} {}x{} fnv={hash:016x} fake={state:?}",size.0,size.1);
    }
    assert_eq!(j.runtime.frames,frame);
    assert_eq!(j.runtime.globals().unwrap().unwrap().clock.ticks(),ticks);
    assert_eq!(game(&j.runtime).and_then(|g|g.reference_fake_select_state()),state,
        "Pure presentation must not resample automatic selection jitter");
}
// Automated canonical keyboard Ctrl (temporary skip) during ordinary story playback.
// This is not physical HID or human-window acceptance.
// Never presses J, never assigns modes/read policy, never fakes a voice result.
fn story_step(j:&mut Journey,viewport:InputViewport,deadline:Instant,enter_was_down:&mut bool){
    assert!(Instant::now()<deadline,"Minagoroshi story deadline");
    let mut raw=key(KeyCode::ControlLeft);
    let mut action=None;
    if let Some(g)=game(&j.runtime){
        assert_eq!(g.reference_play_modes()&2,0,"No latched skip");
        if let Some((phase,selected,rows))=g.reference_story_choice(){
            assert!(g.host().is_some_and(|h|matches!(h.runner.pending_command(),
                Some(Cmd::TIPSGET{..}))),
                "Only declared TIPSGET Continue child may be auto-driven; unplanned SELECT rejected");
            if phase==2&&g.input_permission&&!*enter_was_down{
                let original0=rows.iter().position(|(id,_)|*id==0)
                    .expect("TIPSGET original0 Continue must be visible");
                action=Some(if selected==original0 as i32{KeyCode::Enter}else{KeyCode::ArrowDown});
            }
        }else if matches!(g.reference_story_notification(),Some((12..=14,2)))
            &&!*enter_was_down{
            action=Some(KeyCode::Enter);
        }
    }
    // One release callback follows every arrow/A action, with Ctrl retained.
    *enter_was_down=action.is_some();
    if let Some(code)=action{raw.keyboard.insert(code);}
    assert!(matches!(j.step(&raw,viewport),Frame::Advanced(_)));
}

#[test]
#[ignore="DRAFT exact fixture-unlocked template, normalCategory2/picker2row3/303, deliberateSELECT0 and naturalFakeSelect voice sequence/endTIP143/EXITTitle; requires review, getter, registration and current build; offscreenMetal/audio"]
fn higurashi_runtime_minagoroshi_unlocked_template_observes_automatic_selection_and_full_end(){
    let(saved,initial)=template();
    let capture_dir=PathBuf::from(std::env::var("SHIN_HIGURASHI_MINAGO_CAPTURE")
        .expect("Parent-owned retained capture directory required"));
    std::fs::create_dir_all(&capture_dir).unwrap();
    let expected_settings=serde_json::to_vec(&saved.settings).unwrap();
    let deadline=Instant::now()+Duration::from_secs(2*60*60);
    let viewport=viewport((1920,1080));
    let mut j=Journey::new("minagoroshi-template-features",&saved);
    j.boot_title(viewport,deadline);
    let g=j.runtime.globals().unwrap().unwrap();
    let before=g.runtime_saves.unwrap().clone();
    let ids=[Arc::as_ptr(g.common)as usize,Arc::as_ptr(g.server)as usize,
        Arc::as_ptr(g.manager)as usize,Arc::as_ptr(g.sounds)as usize];
    j.no_writes(Some(&initial));
    if j.runtime.slot().unwrap().title.as_ref().unwrap().reference_top_selection()==Some(1){
        j.tap(KeyCode::ArrowLeft,viewport);
    }
    assert_eq!(j.runtime.slot().unwrap().title.as_ref().unwrap().reference_top_selection(),Some(0));
    j.tap(KeyCode::Enter,viewport);
    j.until(viewport,deadline,"Category0",|r|ready_menu(r,"cat"));
    assert_eq!(j.runtime.slot().unwrap().title.as_ref().unwrap().reference_category(),Some((0,0,2)));
    for wanted in [1,2]{
        j.tap(KeyCode::ArrowDown,viewport);
        assert_eq!(j.runtime.slot().unwrap().title.as_ref().unwrap().reference_category(),Some((0,wanted,2)));
    }
    j.tap(KeyCode::Enter,viewport);
    j.until(viewport,deadline,"Category2 actual VM",|r|scenario(r)==Some(2)&&
        game(r).is_some_and(|g|g.host().is_some_and(|h|h.runner.native_vm_attached())));
    let mut enter_down=false;
    while !picker_ready(&j.runtime){story_step(&mut j,viewport,deadline,&mut enter_down);}
    let source=j.runtime.slot().unwrap().scenario_select.as_ref().unwrap()
        .reference_owner().reference_model().data.clone();
    {
        let m=j.runtime.slot().unwrap().scenario_select.as_ref().unwrap().reference_owner().reference_model();
        assert_eq!((m.kind,m.selected,m.count),(2,0,6));assert_ne!(m.mask&(1<<3),0);
        assert_eq!(source.progress.active_entry().scenario_id,2);
    }
    // Same actual two-column picker geometry as other accepted ordinary routes.
    for(code,wanted)in[(KeyCode::ArrowLeft,1),(KeyCode::ArrowDown,3)]{
        j.tap(code,viewport);assert!(picker_ready(&j.runtime));
        assert_eq!(j.runtime.slot().unwrap().scenario_select.as_ref().unwrap()
            .reference_owner().reference_model().selected,wanted);
    }
    j.tap(KeyCode::Enter,viewport);
    j.until(viewport,deadline,"Actual303 VM attachment",|r|scenario(r)==Some(303)&&
        game(r).is_some_and(|g|g.host().is_some_and(|h|h.runner.native_vm_attached())));
    assert_eq!(game(&j.runtime).unwrap().host().unwrap().runner
        .reference_register(Register::from_regular_register(23)),Some(0),"Fresh ordinary303 v23");
    while !pending(&j.runtime,CHOICE_PC){
        if let Some(h)=game(&j.runtime).and_then(|g|g.host()){
            assert!(!matches!(h.runner.pending_command(),Some(Cmd::SELECT{..}|Cmd::CHARSELECT{..}|Cmd::FAKESELECT{})),
                "Unexpected earlier interactive command needs a declared plan");
        }
        story_step(&mut j,viewport,deadline,&mut enter_down);
    }
    // Release physical Ctrl before deliberate normal SELECT input.
    j.step(&RawInputState::new(),viewport);
    j.until(viewport,deadline,"Exact ordinary choice Input",|r|game(r).is_some_and(|g|
        g.input_permission&&g.reference_story_choice().is_some_and(|(p,_,_)|p==2)));
    for _ in 0..4{
        let(_,selected,rows)=game(&j.runtime).unwrap().reference_story_choice().unwrap();
        let wanted=rows.iter().position(|(id,_)|*id==0).expect("Original0 visible");
        if selected==wanted as i32{break;}
        j.tap(KeyCode::ArrowDown,viewport);
    }
    {
        let(_,selected,rows)=game(&j.runtime).unwrap().reference_story_choice().unwrap();
        assert_eq!(rows[selected as usize].0,0,"Deliberate original0");
    }
    capture(&mut j,&capture_dir,"ordinary-choice");j.tap(KeyCode::Enter,viewport);
    // The real closing tween retains the same SELECT command after A.
    // Release Ctrl and await normal closure before rejecting later choices.
    j.until(viewport,deadline,"Ordinary SELECT closing callback",|r|!pending(r,CHOICE_PC));
    while !pending(&j.runtime,FAKE_PC){
        if let Some(h)=game(&j.runtime).and_then(|g|g.host()){
            assert!(!matches!(h.runner.pending_command(),Some(Cmd::SELECT{..}|Cmd::CHARSELECT{..})),
                "Unexpected ordinary choice after original0");
        }
        story_step(&mut j,viewport,deadline,&mut enter_down);
    }
    let initial_fake=game(&j.runtime).unwrap().reference_fake_select_state()
        .expect("Actual FakeSelect overlay published");
    // In this arrival callback fast_forward saw phase0 and is a no-op. Actual
    // first advance installed entrance phase1. Release canonical Ctrl before phase1 poll.
    assert_eq!((initial_fake.0,initial_fake.1,initial_fake.3),(1,3,60f32.to_bits()),
        "No automatic sequence or countdown skipped before canonical keyboard release");
    j.step(&RawInputState::new(),viewport);
    assert_eq!(game(&j.runtime).unwrap().reference_effective_play_modes()&2,0);
    let fake_deadline=Instant::now()+Duration::from_secs(120);
    let mut playing=[false;2];let mut gated=[false;2];
    let mut stages=Vec::new();let mut previous=3;let mut captured=false;
    while pending(&j.runtime,FAKE_PC){
        assert!(Instant::now()<fake_deadline,"Automatic sequence/natural voice EOF timeout");
        let g=game(&j.runtime).unwrap();assert_eq!(g.reference_effective_play_modes()&2,0);
        let Some(state)=g.reference_fake_select_state()else{break;};
        let voice=g.host().unwrap().audio.status_bits();
        if state.0==2&&(1..=2).contains(&state.1)&&voice==2{
            playing[(2-state.1)as usize]=true;
        }
        if state.1!=previous{stages.push(state.1);previous=state.1;}
        if state.0==2&&state.1==1&&(30f32..100f32).contains(&f32::from_bits(state.3))&&!captured{
            capture(&mut j,&capture_dir,"fake-select-jitter");captured=true;
        }
        // Real audio progress only; neutral full Runtime callbacks, no injected
        // ambient voice, Stop command, status replacement, callback or delta.
        std::thread::sleep(Duration::from_millis(10));
        assert!(matches!(j.step(&RawInputState::new(),viewport),Frame::Advanced(_)));
        if let Some(after)=game(&j.runtime).and_then(|g|g.reference_fake_select_state()){
            let after_voice=game(&j.runtime).unwrap().host().unwrap().audio.status_bits();
            if state.0==2&&after.0==2&&state.1==after.1&&voice==2&&after_voice==2&&(1..=2).contains(&state.1){
                assert_eq!(state.3,after.3,"Live voice busy holds automatic countdown");
                gated[(2-state.1)as usize]=true;
            }
        }
    }
    assert!(captured&&playing.iter().all(|x|*x)&&gated.iter().all(|x|*x));
    assert_eq!(stages,[2,1,0],"Automatic three-move sequence, no row choice result");
    assert!(game(&j.runtime).unwrap().reference_fake_select_state().is_none());
    assert_eq!(game(&j.runtime).unwrap().host().unwrap().audio.status_bits(),0);
    unchanged(&j,&before,&expected_settings,ids);
    let mut final_tip=false;let mut final_exit=None;let mut saw_third_epilogue=false;
    while !ready_menu(&j.runtime,"top"){
        if let Some(g)=game(&j.runtime){if let Some(h)=g.host().filter(|h|h.runner.native_vm_attached()){
            assert_eq!(scenario(&j.runtime),Some(303),"No early replacement/other ending accepted");
            if h.runner.position().0==0xb17e49&&matches!(h.runner.pending_command(),Some(Cmd::TIPSGET{..})){
                assert_eq!(h.runner.reference_register(Register::from_regular_register(23)),Some(0),
                    "Ordinary full chapter, not Chart/section entry");
                final_tip=true;
            }
            saw_third_epilogue|=(0x60166..0x6247f).contains(&h.runner.position().0);
            if matches!(h.runner.pending_command(),Some(Cmd::EXIT{arg1:0,..})){
                assert_eq!(h.runner.reference_register(Register::from_regular_register(23)),Some(0));
                final_exit=Some((h.runner.position().0,h.runner.completed_commands()));
            }
            assert!(!matches!(h.runner.pending_command(),Some(Cmd::SELECT{..}|Cmd::CHARSELECT{..}|Cmd::FAKESELECT{})));
        }}
        story_step(&mut j,viewport,deadline,&mut enter_down);
    }
    assert!(final_tip,"Actual finalTIP143, not early return");
    if (13..=16).all(|slot|saved.persist_data.get(slot)>1){
        assert!(saw_third_epilogue,"Unlocked template invokes actual third-collection epilogue");
    }
    assert_eq!(final_exit.map(|x|x.0),Some(0x1f997),"Actual ordinary final EXIT");
    unchanged(&j,&before,&expected_settings,ids);
    assert_eq!(j.storage.writes.lock().unwrap().len(),1,"One real final storage write");
    capture(&mut j,&capture_dir,"returned-title");
    eprintln!("MINAGO_PASS fixture_unlocked303 original0/FakeSelect naturalvoices/cachedjitter/finalTIP143/EXIT={final_exit:?}/Title; no earned-progression or native-whole-frame claim");
}
