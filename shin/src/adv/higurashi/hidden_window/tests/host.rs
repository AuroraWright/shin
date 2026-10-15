use crate::adv::{
    higurashi::{
        audio::{AudioFramePolicy,HigurashiAudio}, checkpoints::CheckpointServices,
        frame::tests::gpu::render_to_texture, host::HigurashiHost,
        input::{Buttons,Context,InputState,Request}, message_glyphs::tests::gpu::gpu_assets,
        read_history::MessageReadHistory, read_state::ReadPolicy,
        replay::tests::{scenario,session}, runner::{HigurashiRunner,Policy,Status},
        scene_textures::HigurashiSceneTextures,
    }, input_area::{PointerEvent,PointerFrame,PointerKind},
};
use shin_core::{time::Ticks,primitives::color::UnormColor};
use shin_render::shaders::types::texture::{TextureTarget,TextureTargetKind};
use std::sync::Arc;

#[test]
#[ignore = "real hidden message/modal clocks and pixels on offscreen Metal/exact Loopback Audio"]
fn higurashi_host_hidden_window_retires_before_restore_and_keeps_message_resources() {
    let (device,queue,assets)=gpu_assets();let assets=Arc::new(assets);
    let script=scenario(&[0x86,1,0,0,1,0x7f,2,0,b'A',0,0,0,0]);
    let mut initial=session(script.clone(),0,0,42);
    let runner=shin_tasks::block_on(HigurashiRunner::load(assets.clone(),script.clone(),0,42,||[1;4])).unwrap();
    let audio=HigurashiAudio::new(Arc::new(shin_audio::AudioManager::new_for_tests()),assets,script);
    let mut host=HigurashiHost::new(runner,audio);
    let policy=Policy{animations_enabled:true,paused:false,fast_skip:false,suppress_transient_audio:false,read_flags:0,text_speed:50,outline_mode:0};
    let read=ReadPolicy{modes:0,forced:false,read_only:true};
    let audio_policy=AudioFramePolicy{focus_mode_active:true,voice_focus_enabled:true,effects_update_enabled:true,music_rate:1.0};
    let mut history=MessageReadHistory::new();
    let mut services=CheckpointServices{memory:&mut initial.memory,progress:&mut initial.progress,history:&mut history};
    assert_eq!(shin_tasks::block_on(host.runner.pump_native(policy,read,&mut services,8,10000,||[1;4],&mut host.audio)).unwrap(),Status::Waiting);
    host.runner.advance(Ticks::from_f32(15.0),policy,&mut host.audio,None);
    assert!(host.runner.frame().message.playback.interested_in_input());
    let glyphs=host.runner.frame().message.glyphs().unwrap() as *const _;
    let context=Context{permission:true,captured:false,disabled:false,system_locked:false,vm:None,paused:false,continuation_blocked:false,overlay_transition_busy:false,overlay_transition_value:0.0,notification_busy:false,animations_enabled:true,overlay:None,hint_busy:false,scene_jump_available:false,wait_allow_interrupt:0};
    let mut state=InputState{modes:0,temporary_indicator:9};
    let pixels=|host:&HigurashiHost,size| render_to_texture(&device,&queue,size,|context,view|{
        let mut screens=HigurashiSceneTextures::new(context);screens.prepare_scene(context,&host.runner.frame().scene).unwrap();
        let mut pass=context.begin_pass(TextureTarget{view,kind:TextureTargetKind::RenderTexture},Some(context.depth_stencil),"Higurashi hidden message");
        pass.clear(Some(UnormColor::BLACK),Some(0),Some(1.0));screens.render_frame(&mut pass,host.runner.frame()).unwrap();
    });
    for size in [(1280,720),(1920,1080)] { assert!(pixels(&host,size).chunks_exact(4).any(|p|p[..3]!=[0,0,0])); }
    let hidden=host.process_input_native(&mut state,context,Buttons{pressed:1<<25,..Default::default()},&mut services).unwrap();
    assert!(hidden.requests.is_empty());assert_eq!(hidden.overlay_visibility,[true]);assert_eq!(state.temporary_indicator,0);
    assert!(host.has_modes());assert!(host.runner.frame().message.playback.hidden);assert!(host.ui.indicator.hidden);assert!(host.ui.menu.indicator.hidden);
    let position=host.runner.position();
    for elapsed in [3.0,3.0,3.0,1.0] {
        assert_eq!(shin_tasks::block_on(host.update_native(Ticks::from_f32(elapsed),policy,read,&mut services,audio_policy,8,10000,||[1;4])).unwrap(),Status::Waiting);
    }
    assert_eq!(host.runner.position(),position);assert_eq!(host.runner.frame().message.playback.modal_slide,0.0);assert_eq!(host.ui.indicator.opacity,0.0);
    assert_eq!(host.runner.frame().message.glyphs().unwrap() as *const _,glyphs);
    for size in [(1280,720),(1920,1080)] { assert!(pixels(&host,size).chunks_exact(4).all(|p|p[..3]==[0,0,0])); }
    let empty=|number|PointerFrame{number,time_seconds:number as f64*0.02,events:&[]};
    assert!(!host.poll_modes_with_input(Ticks::from_f32(1.0),1,Buttons{pressed:1<<6,..Default::default()},&empty(1)).unwrap());
    assert!(host.runner.frame().message.playback.hidden);
    assert!(!host.poll_modes_with_input(Ticks::from_f32(1.0),1,Buttons{pressed:1<<25,..Default::default()},&empty(2)).unwrap());
    assert!(host.has_modes());assert!(host.runner.frame().message.playback.hidden);
    let blocked=host.process_input_native(&mut state,context,Buttons::default(),&mut services).unwrap();assert_eq!(blocked.requests,[Request::FlushInput]);
    assert!(host.poll_modes_with_input(Ticks::from_f32(1.0),1,Buttons::default(),&empty(3)).unwrap());
    let restored=host.process_input_native(&mut state,context,Buttons::default(),&mut services).unwrap();assert_eq!(restored.overlay_visibility,[false]);
    assert!(!host.has_modes());assert!(!host.runner.frame().message.playback.hidden);assert!(!host.ui.indicator.hidden);assert!(!host.ui.menu.indicator.hidden);
    for elapsed in [3.0,3.0,3.0,1.0] { host.runner.advance(Ticks::from_f32(elapsed),policy,&mut host.audio,None);host.ui.advance(Ticks::from_f32(elapsed)); }
    assert_eq!(host.runner.frame().message.playback.modal_slide,1.0);
    for size in [(1280,720),(1920,1080)] { assert!(pixels(&host,size).chunks_exact(4).any(|p|p[..3]!=[0,0,0])); }
    assert_eq!(host.runner.frame().message.glyphs().unwrap() as *const _,glyphs);
    // Pointer cancel preserves concealment; a subsequent tap completes and
    // retires at the next poll, without consuming the retained message wait.
    host.process_input_native(&mut state,context,Buttons{pressed:1<<25,..Default::default()},&mut services).unwrap();
    for (number,kind) in [(4,PointerKind::Down),(5,PointerKind::Cancel),(6,PointerKind::Down),(7,PointerKind::Up)] {
        let events=[PointerEvent{kind,id:4,position:glam::ivec2(100,100)}];
        let frame=PointerFrame{number,time_seconds:number as f64*0.02,events:&events};
        assert!(!host.poll_modes_with_input(Ticks::from_f32(1.0),1,Buttons::default(),&frame).unwrap());
        assert!(host.runner.frame().message.playback.hidden);
    }
    assert!(host.poll_modes_with_input(Ticks::from_f32(1.0),1,Buttons::default(),&empty(8)).unwrap());
    assert!(!host.runner.frame().message.playback.hidden);assert!(!host.has_modes());
    assert_eq!(host.runner.position(),position);assert_eq!(host.runner.frame().message.glyphs().unwrap() as *const _,glyphs);
    println!("Higurashi hidden host:real MSGWAIT/resource retention, counted visibility, hidden/restored pixels at720p/1080p, button6 exclusion, completion-frame gate, next-poll restore and pointer cancel/tap pass");
}
