//! SOURCE-ONLY candidate: intended child of runtime::tests::journeys::sustained.
//! Not registered, compiled or run. Root must review/admit current integration.
//! Fixture-unlocked template, never claimed as earned chapter progression.
use super::*;

const TEMPLATE_BYTES: usize = 31748;
const TEMPLATE_FNV: u64 = 0x83007166ab0dfcf0;

fn template() -> (SaveData, Vec<u8>) {
    let path = std::env::var("SHIN_HIGURASHI_ORIGIN_TEMPLATE").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"),
            "/../reverse-engineering/higurashi/chapter-playthrough-20261015/unlocked-input.dat").into()
    });
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), TEMPLATE_BYTES);
    let fnv = bytes.iter().fold(0xcbf29ce484222325u64,
        |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3));
    assert_eq!(fnv, TEMPLATE_FNV, "Exact approved unlocked template only");
    let saved = SaveData::decode(&bytes).unwrap();
    assert_eq!(saved.encode().unwrap(), bytes);
    assert!(saved.persist_data.get(176) >= 1, "Ordinary Origin picker enable");
    (saved, bytes)
}
fn kakera_ready(runtime: &Runtime) -> bool {
    runtime.slot().and_then(|s| s.kakera.as_ref())
        .is_some_and(|k| k.reference_input_ready())
}
fn origin_ready(runtime: &Runtime) -> bool {
    runtime.slot().and_then(|s| s.scenario_select.as_ref())
        .is_some_and(|p| {
            let o = p.reference_owner();
            let state = o.reference_state();
            let m = o.reference_model();
            state.0 && state.2 && state.3.is_none() && m.ready && !m.closing
        })
}
fn scenario(runtime: &Runtime) -> Option<i32> {
    game(runtime)?.runtime_game_data().ok()
        .map(|d| d.progress.active_entry().scenario_id)
}
fn menu_snapshot(runtime: &Runtime) -> (usize, usize, Vec<Vec<usize>>, Vec<i32>) {
    let m = &runtime.slot().unwrap().kakera.as_ref().unwrap().motion.model;
    assert!(m.alternate, "Actual Origin fragment menu, not standard catalog");
    (m.page, m.selected, m.pages.clone(), m.statuses.clone())
}
fn select_card(j: &mut Journey, viewport: InputViewport, deadline: Instant, id: usize) {
    let (_, _, pages, statuses) = menu_snapshot(&j.runtime);
    assert!(statuses[id] > 0, "Cannot select locked card{id}");
    let (page, cell) = pages.iter().enumerate().find_map(|(p, ids)| {
        ids.iter().position(|&x| x == id).map(|c| (p,c))
    }).expect("Actual visible card");
    let original_page = menu_snapshot(&j.runtime).0;
    // J = R page-forward. These are approved raw controls; no model writes.
    for _ in 0..pages.len() {
        let (current, _, _, _) = menu_snapshot(&j.runtime);
        if current == page { break; }
        j.tap(KeyCode::KeyJ, viewport);
        j.until(viewport, deadline, "Kakera actual page animation", kakera_ready);
    }
    assert_eq!(menu_snapshot(&j.runtime).0, page);
    let (_, start, _, _) = menu_snapshot(&j.runtime);
    let count = pages[page].len();
    let mut queue = std::collections::VecDeque::from([(start, Vec::<KeyCode>::new())]);
    let mut seen = std::collections::BTreeSet::from([start]);
    let plan = loop {
        let (at, path) = queue.pop_front().expect("Native cell navigation is connected");
        if at == cell { break path; }
        for code in [KeyCode::ArrowUp, KeyCode::ArrowDown,
            KeyCode::ArrowLeft, KeyCode::ArrowRight] {
            let (mut row, mut col) = (at / 3, at % 3);
            match code {
                KeyCode::ArrowUp | KeyCode::ArrowDown => {
                    loop { row = (row + 1) % 2; if 3*row+col < count { break; } }
                }
                KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                    let amount = if code == KeyCode::ArrowLeft { 2 } else { 1 };
                    loop { col = (col + amount) % 3; if 3*row+col < count { break; } }
                }
                _ => unreachable!(),
            }
            let next = 3*row+col;
            if seen.insert(next) {
                let mut path = path.clone(); path.push(code); queue.push_back((next,path));
            }
        }
    };
    let mut expected = start;
    for code in plan {
        let (mut row, mut col) = (expected / 3, expected % 3);
        if matches!(code,KeyCode::ArrowUp|KeyCode::ArrowDown) {
            loop {row=(row+1)%2;if 3*row+col<count{break;}}
        } else {
            let d=if code==KeyCode::ArrowLeft{2}else{1};
            loop {col=(col+d)%3;if 3*row+col<count{break;}}
        }
        expected=3*row+col;
        j.tap(code,viewport);
        assert!(kakera_ready(&j.runtime));
        assert_eq!(menu_snapshot(&j.runtime).1,expected,"Actual native cell navigation");
    }
    let k = j.runtime.slot().unwrap().kakera.as_ref().unwrap();
    assert_eq!(k.motion.model.selected_fragment(), id);
    if page == 8 && original_page != 8 { pure_frame(j); }
    j.tap(KeyCode::Enter, viewport);
}

fn preserved(j:&Journey, before:&crate::app::higurashi::runtime_saves::Slots,
    settings:&[u8], ids:[usize;4]) {
    let now=j.runtime.globals().unwrap().unwrap();
    assert_eq!(serde_json::to_vec(&now.data.settings).unwrap(),settings);
    assert_eq!([Arc::as_ptr(now.common)as usize,Arc::as_ptr(now.server)as usize,
        Arc::as_ptr(now.manager)as usize,Arc::as_ptr(now.sounds)as usize],ids);
    for (i,(a,b)) in now.runtime_saves.unwrap().manual.iter().zip(&before.manual).enumerate(){
        assert_eq!(a.valid,b.valid,"manual{i} validity");
        assert_eq!(a.data,b.data,"manual{i} complete payload");
    }
}

#[test]
#[ignore = "DRAFT not admitted; exact isolated unlocked template -> normal Top8/Origin7/10008 -> authored Kakera/card/Bonus10010 route; requires review/registration/current build; no earned-clear claim"]
fn higurashi_runtime_origin10008_unlocked_template_drives_real_fragment_progression() {
    let (saved, original) = template();
    let expected_settings=serde_json::to_vec(&saved.settings).unwrap();
    let mut j = Journey::new("origin10008-template-features", &saved);
    let viewport = viewport((1920,1080));
    let deadline = Instant::now() + Duration::from_secs(2 * 60 * 60);
    j.boot_title(viewport, deadline);
    let before = j.runtime.globals().unwrap().unwrap().runtime_saves.unwrap().clone();
    let original_globals = {
        let g = j.runtime.globals().unwrap().unwrap();
        [Arc::as_ptr(g.common) as usize, Arc::as_ptr(g.server) as usize,
         Arc::as_ptr(g.manager) as usize, Arc::as_ptr(g.sounds) as usize]
    };
    j.no_writes(Some(&original));
    // The exact template may have an auto row; ordinary Title then defaults1.
    let top = j.runtime.slot().unwrap().title.as_ref().unwrap()
        .reference_top_selection().unwrap();
    if top == 1 { j.tap(KeyCode::ArrowLeft, viewport); }
    assert_eq!(j.runtime.slot().unwrap().title.as_ref().unwrap()
        .reference_top_selection(), Some(0));
    j.tap(KeyCode::ArrowUp, viewport);
    assert_eq!(j.runtime.slot().unwrap().title.as_ref().unwrap()
        .reference_top_selection(), Some(8));
    j.tap(KeyCode::Enter, viewport);
    j.until(viewport, deadline, "normal Origin picker Input", origin_ready);
    {
        let m = j.runtime.slot().unwrap().scenario_select.as_ref().unwrap().reference_owner().reference_model();
        assert_eq!((m.kind,m.selected,m.count), (5,0,10));
        assert_ne!(m.mask & (1 << 7), 0);
    }
    for (key,wanted) in [(KeyCode::ArrowLeft,1),(KeyCode::ArrowDown,3),
        (KeyCode::ArrowDown,5),(KeyCode::ArrowDown,7)] {
        j.tap(key,viewport);
        assert!(origin_ready(&j.runtime));
        assert_eq!(j.runtime.slot().unwrap().scenario_select.as_ref().unwrap().reference_owner()
            .reference_model().selected,wanted);
    }
    j.tap(KeyCode::Enter,viewport);
    j.until(viewport,deadline,"actual10008 VM attachment",|r| {
        scenario(r)==Some(10008) && game(r).is_some_and(|g|g.host()
            .is_some_and(|h|h.runner.native_vm_attached()))
    });
    let mut driver = Driver::new();
    while !kakera_ready(&j.runtime) {
        if let Some(h)=game(&j.runtime).and_then(|g|g.host()) {
            assert!(!matches!(h.runner.pending_command(),Some(Cmd::SELECT{..}|Cmd::CHARSELECT{..})),
                "Ordinary v22=0 route must bypass NCSELECT; explicit unexpected choice plan required");
        }
        driver.step(&mut j,viewport,deadline);
    }
    preserved(&j,&before,&expected_settings,original_globals);
    pure_frame(&mut j);
    {
        let m = &j.runtime.slot().unwrap().kakera.as_ref().unwrap().motion.model;
        assert_eq!(m.data.progress.entries[0].scenario_id,10008);
        assert!(m.data.progress.entries[0].save_position > 0);
        assert_eq!(m.statuses.len(),54);
        for id in [8,17,22,25,29,38] {
            assert!(m.statuses[id] > 0,"Initial prologue byte-backed card{id}");
        }
    }
    let mut completed_cards = std::collections::BTreeSet::new();
    let mut saw_bonus = false;
    let mut saw_main_exit = false;
    let mut saw_main_entry = false;
    let mut main_capture = false;
    for _round in 0..56 {
        assert!(kakera_ready(&j.runtime));
        let (_,_,pages,statuses)=menu_snapshot(&j.runtime);
        // Prioritize newly available unread ordinary fragments; optional hints
        // are last. Reading a status is observation, never a response override.
        // Native fragment49 first unlocks50. Once50 is actually read,
        // reread49 to take its byte50-dependent alternate ending and set52=2;
        // only then EXIT12049 can legitimately produce Bonus10010.
        let bytes = &j.runtime.slot().unwrap().kakera.as_ref().unwrap()
            .motion.model.data.byte_variables;
        let revisit49 = statuses[49] >= 2 && statuses[50] >= 2
            && bytes.get(52).copied().unwrap_or(0) < 2;
        let id = if revisit49 { Some(49) } else {
            (0..=50).find(|id| {
                statuses[*id] > 0 && !completed_cards.contains(id)
                    && pages.iter().any(|p|p.contains(id))
            })
        };
        let Some(id)=id else {
            eprintln!("ORIGIN_BOUNDARY open_fragment_loop cards={completed_cards:?} statuses={statuses:?}");
            panic!("No unvisited available fragment; retain real loop as incomplete, never claim full-end");
        };
        select_card(&mut j,viewport,deadline,id);
        j.until(viewport,deadline,"selected actual fragment VM attachment",|r| {
            scenario(r)==Some(12000+id as i32) && game(r).is_some_and(|g|g.host()
                .is_some_and(|h|h.runner.native_vm_attached()))
        });
        let mut saw_notification=false;
        loop {
            assert!(Instant::now()<deadline);
            if kakera_ready(&j.runtime) { break; }
            if ready_menu(&j.runtime,"top") {
                assert!(saw_bonus && saw_main_entry && saw_main_exit,
                    "Do not mistake direct12051/hint return for full10010 ending");
                preserved(&j,&before,&expected_settings,original_globals);
                let writes=j.storage.writes.lock().unwrap();
                assert_eq!(writes.len(),1,"Exactly actual final EXIT storage write");
                assert!(SaveData::decode(&writes[0]).is_ok());
                drop(writes);
                let now=j.runtime.globals().unwrap().unwrap();
                let actual=[Arc::as_ptr(now.common) as usize,Arc::as_ptr(now.server) as usize,
                    Arc::as_ptr(now.manager) as usize,Arc::as_ptr(now.sounds) as usize];
                assert_eq!(actual,original_globals);
                let rows=now.runtime_saves.unwrap();
                for (i,(a,b)) in rows.manual.iter().zip(&before.manual).enumerate() {
                    assert_eq!(a.valid,b.valid,"manual{i} validity");
                    assert_eq!(a.data,b.data,"manual{i} full retained payload");
                }
                assert!(now.data.persist_data.get(176)&2 != 0);
                pure_frame(&mut j);
                eprintln!("ORIGIN_TERMINAL fixture_unlocked10008+Kakera+actualBonus10010 cards={completed_cards:?}; no earned progression/complete branch roster claim");
                return;
            }
            if let Some(g)=game(&j.runtime) {
                if let Some(h)=g.host().filter(|h|h.runner.native_vm_attached()) {
                    if scenario(&j.runtime)==Some(10010) {
                        saw_bonus=true;
                        saw_main_entry |= h.runner.native_save_metadata().is_some_and(|m|
                            m.saved()[0].as_ref()=="ひぐらしオリジン・祭囃し");
                    }
                    if h.runner.position().0==0x1823731
                        && matches!(h.runner.pending_command(),Some(Cmd::EXIT{..})) {
                        assert_eq!(scenario(&j.runtime),Some(10010));
                        saw_main_exit=true;
                    }
                    saw_notification |= matches!(h.runner.pending_command(),Some(Cmd::KAKERAGET{..}));
                    assert!(!matches!(h.runner.pending_command(),Some(Cmd::SELECT{..}|Cmd::CHARSELECT{..})),
                        "Unexpected interaction requires an explicit plan");
                }
            }
            if saw_main_entry && !main_capture {
                pure_frame(&mut j); main_capture=true;
            }
            driver.step(&mut j,viewport,deadline);
        }
        completed_cards.insert(id);
        let (_,_,_,after)=menu_snapshot(&j.runtime);
        assert!(after[id]>=2,"Actual completed fragment card{id}");
        preserved(&j,&before,&expected_settings,original_globals);
        if completed_cards.len()==1 {pure_frame(&mut j);}
        eprintln!("ORIGIN_CARD id={id} status={} notification_observed={saw_notification} visited={}",after[id],completed_cards.len());
    }
    panic!("Bounded card plan exhausted without verified main-body terminal");
}
