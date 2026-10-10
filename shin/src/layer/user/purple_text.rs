//! Exact UTF-8 purple-statement text from the supplied Switch executable.
//! Chapter table: 0x1d9270; person text pointers: 0x1d9528.
//! Keep native @r/@c/@z markup for the engine text layouter.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_chapter_headings_and_label_rows_are_distinct() {
        let headings: Vec<_> = CHAPTERS
            .iter()
            .filter_map(|entry| {
                entry
                    .heading()
                    .map(|heading| (entry.original_index, heading))
            })
            .collect();
        assert_eq!(
            headings,
            [(0, 0), (6, 1), (14, 2), (19, 3), (23, 4), (26, 5)]
        );
        let labels: Vec<_> = CHAPTERS
            .iter()
            .filter_map(PurpleChapter::label_row)
            .collect();
        assert_eq!(labels, (0..23).collect::<Vec<_>>());
        let red_truth: Vec<_> = CHAPTERS
            .iter()
            .filter(|entry| entry.is_red_truth())
            .map(|entry| entry.original_index)
            .collect();
        assert_eq!(red_truth, [5, 11, 12, 13, 18]);
        for entry in &CHAPTERS {
            assert_eq!(entry.text.is_empty(), entry.heading().is_some());
        }
    }

    #[test]
    fn native_person_order_and_empty_transcripts_are_preserved() {
        assert_eq!(
            PERSON_KEYS,
            [
                "kla", "nat", "jes", "", "", "geo", "", "", "but", "", "mar", "nan", "", "sha",
                "kan", "goh", "kum"
            ]
        );
        for index in 0..17 {
            assert_eq!(PERSON_TEXT[index].is_empty(), PERSON_KEYS[index].is_empty());
        }
        assert!(CHAPTERS[1].text.starts_with("郷田@r「@c649."));
        assert!(PERSON_TEXT[0].starts_with("@z110.■第一の晩@z100.@r"));
        assert!(CHAPTERS[5].text.starts_with("@c900."));
    }

    #[test]
    #[ignore = "requires supplied uncompressed executable and extracted reference font metrics"]
    fn reference_purple_text_matches_native_tables_and_parses_with_reference_fonts() {
        use shin_core::layout::{
            LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
        };
        use std::{collections::HashMap, fs::File, io::BufReader, path::Path};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let executable =
            std::fs::read(root.join("umineko_switch_files/uncompressed_executable")).unwrap();
        let u64_at = |address: usize| {
            u64::from_le_bytes(executable[address + 256..address + 264].try_into().unwrap())
        };
        let relocations: HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|address| (u64_at(address) as usize, u64_at(address + 16) as usize))
            .collect();
        let native_text = |address: usize| {
            let bytes = &executable[address + 256..];
            let length = bytes.iter().position(|&byte| byte == 0).unwrap();
            std::str::from_utf8(&bytes[..length]).unwrap()
        };
        for entry in &CHAPTERS {
            let table = 0x1d9270 + entry.original_index * 24;
            assert_eq!(u64_at(table) as u32 as usize, entry.original_index);
            assert_eq!((u64_at(table) >> 32) as u8, entry.flags);
            assert_eq!(relocations[&(table + 16)], entry.source_address);
            assert_eq!(native_text(entry.source_address), entry.text);
        }
        for index in 0..17 {
            assert_eq!(
                relocations[&(0x1d9528 + index * 8)],
                PERSON_SOURCE_ADDRESSES[index]
            );
            assert_eq!(
                native_text(PERSON_SOURCE_ADDRESSES[index]),
                PERSON_TEXT[index]
            );
            assert_eq!(
                native_text(relocations[&(0x1d9970 + index * 8)]),
                PERSON_KEYS[index]
            );
        }
        let read_font = |name: &str| {
            let mut reader =
                BufReader::new(File::open(root.join("shin-core/test_assets").join(name)).unwrap());
            shin_core::format::font::read_font_metrics(&mut reader).unwrap()
        };
        let normal = read_font("newrodin-medium.fnt");
        let bold = read_font("newrodin-bold.fnt");
        for text in CHAPTERS.iter().map(|entry| entry.text).chain(PERSON_TEXT) {
            if text.is_empty() {
                continue;
            }
            let (commands, lines, size) = MessageTextLayouter::new(
                &normal,
                &bold,
                LayoutParams {
                    layout_width: 1024.0,
                    text_size: 32.0,
                    ..Default::default()
                },
                MessageTextLayouterDefaults {
                    color: 999,
                    draw_speed: 100,
                    fade: 0,
                },
            )
            .parse(text);
            assert!(size.is_finite() && size.x > 0.0 && size.y > 0.0);
            assert!(!lines.is_empty());
            let glyphs: Vec<_> = commands
                .iter()
                .filter_map(|command| match command {
                    Command::Char(character) => Some(character),
                    _ => None,
                })
                .collect();
            assert!(!glyphs.is_empty());
            assert!(
                glyphs.iter().all(|character| character.codepoint != '@'),
                "Unparsed native markup"
            );
            assert!(
                glyphs
                    .iter()
                    .all(|character| character.position.is_finite()),
                "Invalid glyph placement"
            );
        }
    }
}

#[derive(Debug)]
pub struct PurpleChapter {
    pub original_index: usize,
    pub flags: u8,
    pub source_address: usize,
    pub text: &'static str,
}
impl PurpleChapter {
    pub fn heading(&self) -> Option<usize> {
        (self.flags & 0x80 != 0).then_some((self.flags & 0x3f) as usize)
    }
    pub fn label_row(&self) -> Option<usize> {
        (self.flags & 0x80 == 0).then_some((self.flags & 0x3f) as usize)
    }
    pub fn is_red_truth(&self) -> bool {
        self.flags & 0x40 != 0
    }
}

pub const CHAPTERS: [PurpleChapter; 29] = [
    PurpleChapter {
        original_index: 0,
        flags: 128,
        source_address: 0x162118,
        text: "",
    },
    PurpleChapter {
        original_index: 1,
        flags: 0,
        source_address: 0x15ad80,
        text: "郷田@r「@c649.食堂は施錠されていました。@c999.確かに食堂に鍵は付いていますが、普段、施錠することなどありません。ノックしたのですが返事はなく…」@r@z25.@r@z100.熊沢@r「@c649.そこに私が来たのです。@c999.万一のこともあるから、鍵を開けてみたらと勧めたのです」@r@z25.@r@z100.郷田@r「@c649.マスターキーで扉を開ける@c999.と、中は恐ろしいことに……」@r@z25.@r@z100.熊沢@r「@c649.絵羽さま、秀吉さま、留弗夫さま、霧江さま、楼座さま、そして源次さんの、計６人が血塗れになって倒れていたのでございます……@c999.」@r@z25.@r@z100.蔵臼@r「それで後は大騒ぎだ。@c649.全員が食堂に集まった@c999.」",
    },
    PurpleChapter {
        original_index: 2,
        flags: 1,
        source_address: 0x15edea,
        text: "朱志香@r「ひょっとしたら生きてるかもと。……みんな懸命に揺さぶってたぜ」@r@z25.@r@z100.紗音@r「しかし、駄目でした…。@c649.お子様方はそれぞれ、自分の親が間違いなく死んでいることを確認なさいました…@c999.」@r@z25.@r@z100.嘉音@r「@c649.僕と南條先生は、源次さまが死んでいることを確認しました@c999.」@r@z25.@r@z100.南條@r「惨たらしい殺され方でした……。@c649.私でなくとも、誰も検死は誤りません@c999.」@r@z25.@r@z100.真里亞@r「@c649.犠牲者はみんな、即死@c999.だったろうね。……きひひひひひ」",
    },
    PurpleChapter {
        original_index: 3,
        flags: 2,
        source_address: 0x15f089,
        text: "蔵臼@r「@c649.食堂内を調べた結果、扉も窓も、全て施錠され、密室であったことがわかった@c999.」@r@z25.@r@z100.戦人@r「そして、@c649.食堂内から、不審なものは何も見つからなかった@c999.」@r@z25.@r@z100.譲治@r「もちろん、@c649.食堂内に何者かが隠れているということもなかったよ@c999.」@r@z25.@r@z100.蔵臼@r「隠れるも何も。私たちは今ここに全員がいるではないかね。@c649.誰も隠れていないことは明白だ@c999.」",
    },
    PurpleChapter {
        original_index: 4,
        flags: 3,
        source_address: 0x15d6a3,
        text: "夏妃@r「後のトラブルを予防するため、@c649.源次のマスターキーを破壊しておきました@c999.」",
    },
    PurpleChapter {
        original_index: 5,
        flags: 68,
        source_address: 0x162464,
        text: "@c900.全ての扉の施錠、開錠は、マスターキーでしか行えない。@r@z25.@r@z100.もちろん、部屋の内側からはマスターキーがなくとも、施錠、開錠が可能。@r@z25.@r@z100.マスターキー以外の鍵は存在しないこととする。@r@z25.@r@z100.マスターキーは合計５本。５人の使用人が１本ずつを持ってる。@r@z25.@r@z100.マスターキーは常に身に付けられており、奪うことも譲渡することも、自分以外の人間が使用することも出来ない。@c999.",
    },
    PurpleChapter {
        original_index: 6,
        flags: 129,
        source_address: 0x162118,
        text: "",
    },
    PurpleChapter {
        original_index: 7,
        flags: 5,
        source_address: 0x15a993,
        text: "郷田@r「@c649.奥様の部屋は施錠されておりました！@c999.　食堂の時と同じです！」@r@z25.@r@z100.紗音@r「皆さんの許可を得て、@c649.私が鍵を開けました…@c999.」@r@z25.@r@z100.嘉音@r「@c649.部屋の中には、蔵臼さまと夏妃さまが倒れていました@c999.」",
    },
    PurpleChapter {
        original_index: 8,
        flags: 6,
        source_address: 0x15d09e,
        text: "戦人@r「@c649.南條先生がすぐ脈を取ったぜ。そして、二人とも即死だったと宣言した@c999.」@r@z25.@r@z100.南條@r「はい。@c649.私が二人の死亡を確認しました。あれは間違いなく即死でした@c999.」@r@z25.@r@z100.朱志香@r「私は何かの手掛りがないか部屋中を探し回ったよ…！　その結果、@c649.窓も扉も完全に施錠されていて、密室だったことがわかった@c999.」",
    },
    PurpleChapter {
        original_index: 9,
        flags: 7,
        source_address: 0x1633b9,
        text: "紗音@r「@c649.私たち使用人全員は、常に一緒におりました@c999.」@r@z25.@r@z100.嘉音@r「@c649.僕たち使用人全員は、それぞれが使用人全員のアリバイを証明できます@c999.」",
    },
    PurpleChapter {
        original_index: 10,
        flags: 8,
        source_address: 0x15a4b3,
        text: "真里亞@r「@c649.外からガムテープでベタベタと、窓や扉を封印したよ！@c999.」@r@z25.@r@z100.朱志香@r「きっとこの部屋に、犯人の何かの手掛りが残ってるはず！　@c649.警察が来るまで絶対、誰の出入りも出来ないようにした！@c999.」@r@z25.@r@z100.戦人@r「となりゃ、食堂も同じだ。@c649.俺たちはさらに食堂も同様に封印した@c999.」@r@z25.@r@z100.南條@r「そもそも、屋敷自体を保全するべきではということになりましてな。@c649.屋敷自体にも封印をし、私たちは全員、ゲストハウスへ避難しました@c999.」",
    },
    PurpleChapter {
        original_index: 11,
        flags: 73,
        source_address: 0x15d276,
        text: "@c900.全ての窓には鉄格子が入っており、窓からの出入りは不可能である。@r@z25.@r@z100.玄関と裏口の２つの扉を封印するだけで屋敷は封印できることにする。@c999.",
    },
    PurpleChapter {
        original_index: 12,
        flags: 74,
        source_address: 0x160da0,
        text: "@c900.第一の晩の犯人は確実に６人を殺している。@c999.",
    },
    PurpleChapter {
        original_index: 13,
        flags: 75,
        source_address: 0x15d34f,
        text: "@c900.一同は退出と同時に部屋を封印した。その退出に犯人は加われない。そして、夏妃の部屋、食堂、屋敷の全ての封印は、決して破られることはない。@c999.",
    },
    PurpleChapter {
        original_index: 14,
        flags: 130,
        source_address: 0x162118,
        text: "",
    },
    PurpleChapter {
        original_index: 15,
        flags: 12,
        source_address: 0x15f2a8,
        text: "朱志香@r「譲治兄さんが騒ぎ出し、@c649.全員で外に出て紗音たちを探すことになったんだ@c999.」@r@z25.@r@z100.熊沢@r「紗音さんは薔薇庭園に倒れておりました。おぉ、おいたわしや……」@r@z25.@r@z100.譲治@r「生きててくれと願ったさ。でも、@c649.僕は彼女が死んでいることを認めなければならなかった…@c999.」@r@z25.@r@z100.南條@r「無論、@c649.私も検死し、彼女の死亡を確認しました@c999.」",
    },
    PurpleChapter {
        original_index: 16,
        flags: 13,
        source_address: 0x159bd3,
        text: "郷田@r「二人が出掛けた時、私たちはゲストハウスの戸締りを確認するために色々とやっていました。だから、@c649.私たちにはみんな、アリバイがないのです…！@c999.」@r@z25.@r@z100.譲治@r「僕が殺すものか！　僕に殺せるものか！！」@r@z25.@r@z100.真里亞@r「うん。譲治お兄ちゃんには殺せないね、うー」@r@z25.@r@z100.朱志香@r「その後、譲治兄さんにだけはアリバイがあることがわかったんだ。@c649.紗音殺しに限っては、譲治兄さんには不可能なんだ@c999.」@r@z25.@r@z100.戦人@r「逆に言えば。@c649.……譲治の兄貴以外なら誰にでも殺せたわけだ@c999.」",
    },
    PurpleChapter {
        original_index: 17,
        flags: 14,
        source_address: 0x15bd30,
        text: "郷田@r「犯人に悪用されるのを防ぐため、@c649.紗音さんの持ってたマスターキーは、その場で破壊しました@c999.」",
    },
    PurpleChapter {
        original_index: 18,
        flags: 79,
        source_address: 0x15aad8,
        text: "@c900.以後、嘉音は殺されたものとして扱う。嘉音のマスターキーも破壊されたものとして扱うわ。@c999.",
    },
    PurpleChapter {
        original_index: 19,
        flags: 131,
        source_address: 0x162118,
        text: "",
    },
    PurpleChapter {
        original_index: 20,
        flags: 16,
        source_address: 0x15b0b0,
        text: "南條@r「@c649.郷田さんも熊沢さんも、この傷では即死@c999.だったでしょう。惨たらしい死に方です…」@r@z25.@r@z100.朱志香@r「この傷で生きてるわけあるもんか…！　@c649.死んでるよ、郷田さんも熊沢さんも！@c999.」",
    },
    PurpleChapter {
        original_index: 21,
        flags: 17,
        source_address: 0x1615a5,
        text: "戦人@r「戸締り確認とか見回りとか、色々やってたからな。今回もまた、@c649.俺たちには全員、アリバイがない@c999.」@r@z25.@r@z100.譲治@r「そんなことはないよ。見てごらん。……あんな殺し方をしたら、絶対に犯人は返り血を浴びるはずなんだ」@r@z25.@r@z100.真里亞@r「でも、いとこ全員も、そして南條先生も返り血は浴びてないね」@r@z25.@r@z100.朱志香@r「私たち全員は、……つまり、@c649.いとこ４人と南條先生には、郷田さんと熊沢さんは殺せない@c999.」",
    },
    PurpleChapter {
        original_index: 22,
        flags: 18,
        source_address: 0x159eae,
        text: "譲治@r「何者かの侵入を疑って調べたけれど、@c649.ゲストハウスの戸締りは完璧だったよ@c999.」@r@z25.@r@z100.南條@r「犯人はやはり、マスターキーを持っているのかもしれません…」@r@z25.@r@z100.戦人@r「それはありえねぇぜ。@c649.マスターキーはもう、ここで死んでる２人の２本以外、存在しない@c999.」",
    },
    PurpleChapter {
        original_index: 23,
        flags: 132,
        source_address: 0x162118,
        text: "",
    },
    PurpleChapter {
        original_index: 24,
        flags: 19,
        source_address: 0x15c0ed,
        text: "朱志香@r「@c649.し、死んでる…。殺されてる……！@c999.」@r@z25.@r@z100.譲治@r「僕でさえ、@c649.これは即死だったに違いない@c999.と一目でわかるよ…」@r@z25.@r@z100.真里亞@r「@c649.戸締りは完璧だよ。@c999.密室なのにどうして？」",
    },
    PurpleChapter {
        original_index: 25,
        flags: 20,
        source_address: 0x162f92,
        text: "朱志香@r「それはありえない！　状況から判断して、@c649.私にも真里亞にも、戦人にも譲治兄さんにも、南條先生は殺せない！@c999.」@r@z25.@r@z100.譲治@r「そもそも、@c649.ゲストハウス内で南條先生を殺すことは不可能.@c999.なんだ！」@r@z25.@r@z100.戦人@r「しかも、これを見ろよ。つまりこいつは、@c649.南條先生がゲストハウスを出ていない証拠だ…！@c999.」",
    },
    PurpleChapter {
        original_index: 26,
        flags: 133,
        source_address: 0x162118,
        text: "",
    },
    PurpleChapter {
        original_index: 27,
        flags: 21,
        source_address: 0x1634b2,
        text: "譲治@r「朱志香ちゃん、……可哀想に。@c649.即死だったろう@c999.」@r@z25.@r@z100.真里亞@r「@c649.これで生きているわけがない@c999.もんね。きひひひひ」",
    },
    PurpleChapter {
        original_index: 28,
        flags: 22,
        source_address: 0x160370,
        text: "戦人@r「俺たちは３人でずっと一緒だった！　@c649.俺も譲治の兄貴も真里亞も、朱志香は殺せねぇぞ！@c999.」@r@z25.@r@z100.真里亞@r「うん、@c649.私たち３人に朱志香は殺せない@c999.」@r@z25.@r@z100.譲治@r「真里亞ちゃんが人なんか殺すもんか。@c649.真里亞ちゃんには誰も殺せないよ@c999.」@r@z25.@r@z100.真里亞@r「きひひ、ありがとう。@c649.譲治お兄ちゃんだって、大人は殺せないよ。子供は殺せるけれど。@c999.きひひひひひひ」",
    },
];

pub const PERSON_TEXT: [&str; 17] = [
    "@z110.■第一の晩@z100.@r「それで後は大騒ぎだ。@c649.全員が食堂に集まった@c999.」@r@z25.@r@z100.「@c649.食堂内を調べた結果、扉も窓も、全て施錠され、密室であったことがわかった@c999.」@r@z25.@r@z100.「隠れるも何も。私たちは今ここに全員がいるではないかね。@c649.誰も隠れていないことは明白だ@c999.」",
    "@z110.■第一の晩@z100.@r「後のトラブルを予防するため、@c649.源次のマスターキーを破壊しておきました@c999.」",
    "@z110.■第二の晩@z100.@r「私は何かの手掛りがないか部屋中を探し回ったよ…！　その結果、@c649.窓も扉も完全に施錠されていて、密室だったことがわかった@c999.」@r@z25.@r@z100.「きっとこの部屋に、犯人の何かの手掛りが残ってるはず！　@c649.警察が来るまで絶対、誰の出入りも出来ないようにした！@c999.」@r@z25.@r@z100.@z110.■第四の晩@z100.@r「譲治兄さんが騒ぎ出し、@c649.全員で外に出て紗音たちを探すことになったんだ@c999.」@r@z25.@r@z100.「その後、譲治兄さんにだけはアリバイがあることがわかったんだ。@c649.紗音殺しに限っては、譲治兄さんには不可能なんだ@c999.」@r@z25.@r@z100.@z110.■第五・六の晩@z100.@r「この傷で生きてるわけあるもんか…！　@c649.死んでるよ、郷田さんも熊沢さんも！@c999.」@r@z25.@r@z100.「私たち全員は、……つまり、@c649.いとこ４人と南條先生には、郷田さんと熊沢さんは殺せない@c999.」@r@z25.@r@z100.@z110.■第七の晩@z100.@r「@c649.し、死んでる…。殺されてる……！@c999.」@r@z25.@r@z100.「それはありえない！　状況から判断して、@c649.私にも真里亞にも、戦人にも譲治兄さんにも、南條先生は殺せない！@c999.」",
    "",
    "",
    "@z110.■第一の晩@z100.@r「もちろん、@c649.食堂内何者かが隠れているということもなかったよ@c999.」@r@z25.@r@z100.@z110.■第四の晩@z100.@r「生きててくれと願ったさ。でも、@c649.僕は彼女が死んでいることを認めなければならなかった…@c999.」@r@z25.@r@z100.@z110.■第五・六の晩@z100.@r「何者かの侵入を疑って調べたけれど、@c649.ゲストハウスの戸締りは完璧だったよ@c999.」@r@z25.@r@z100.@z110.■第七の晩@z100.@r「僕でさえ、@c649.これは即死だったに違いない@c999.と一目でわかるよ…」@r@z25.@r@z100.「そもそも、@c649.ゲストハウス内で南條先生を殺すことは不可能なんだ！@c999.」@r@z25.@r@z100.@z110.■第八の晩@z100.@r「朱志香ちゃん、……可哀想に。@c649.即死だったろう@c999.」@r@z25.@r@z100.「真里亞ちゃんが人なんか殺すもんか。@c649.真里亞ちゃんには誰も殺せないよ@c999.」",
    "",
    "",
    "@z110.■第一の晩@z100.@r「そして、@c649.食堂内から、不審なものは何も見つからなかった@c999.」@r@z25.@r@z100.@z110.■第二の晩@z100.@r「@c649.南條先生がすぐ脈を取ったぜ。そして、二人とも即死だったと宣言した@c999.」@r@z25.@r@z100.「となりゃ、食堂も同じだ。@c649.俺たちはさらに食堂も同様に封印した@c999.」@r@z25.@r@z100.@z110.■第四の晩@z100.@r「逆に言えば。@c649.……譲治の兄貴以外なら誰にでも殺せたわけだ@c999.」@r@z25.@r@z100.@z110.■第五・六の晩@z100.@r「戸締り確認とか見回りとか、色々やってたからな。今回もまた、@c649.俺たちには全員、アリバイがない@c999.」@r@z25.@r@z100.「それはありえねぇぜ。@c649.マスターキーはもう、ここで死んでる２人の２本以外、存在しない@c999.」@r@z25.@r@z100.@z110.■第七の晩@z100.@r「しかも、これを見ろよ。つまりこいつは、@c649.南條先生がゲストハウスを出ていない証拠だ…！@c999.」@r@z25.@r@z100.@z110.■第八の晩@z100.@r「俺たちは３人でずっと一緒だった！　@c649.俺も譲治の兄貴も真里亞も、朱志香は殺せねぇぞ！@c999.」",
    "",
    "@z110.■第一の晩@z100.@r「@c649.犠牲者はみんな、即死@c999.だったろうね。……きひひひひひ」@r@z25.@r@z100.@z110.■第二の晩@z100.@r「@c649.外からガムテープでベタベタと、窓や扉を封印したよ！@c999.」@r@z25.@r@z100.@z110.■第七の晩@z100.@r「@c649.戸締りは完璧だよ。@c999.密室なのにどうして？」@r@z25.@r@z100.@z110.■第八の晩@z100.@r「@c649.これで生きているわけがない@c999.もんね。きひひひひ」@r@z25.@r@z100.「うん。@c649.私たち３人に朱志香は殺せない@c999.」@r@z25.@r@z100.「きひひ、ありがとう。@c649.譲治お兄ちゃんだって、大人は殺せないよ。子供は殺せるけれど。@c999.きひひひひひひ」",
    "@z110.■第一の晩@z100.@r「惨たらしい殺され方でした……。@c649.私でなくとも、誰も検死は誤りません@c999.」@r@z25.@r@z100.@z110.■第二の晩@z100.@r「はい。@c649.私が二人の死亡を確認しました。あれは間違いなく即死でした@c999.」@r@z25.@r@z100.「そもそも、屋敷自体を保全するべきではということになりましてな。@c649.屋敷自体にも封印をし、私たちは全員、ゲストハウスへ避難しました@c999.」@r@z25.@r@z100.@z110.■第四の晩@z100.@r「無論、@c649.私も検死し、彼女の死亡を確認しました@c999.」@r@z25.@r@z100.@z110.■第五・六の晩@z100.@r「@c649.郷田さんも熊沢さんも、この傷では即死@c999.だったでしょう。惨たらしい死に方です…」",
    "",
    "@z110.■第一の晩@z100.@r「しかし、駄目でした…。@c649.お子様方はそれぞれ、自分の親が間違いなく死んでいることを確認なさいました…@c999.」@r@z25.@r@z100.@z110.■第二の晩@z100.@r「皆さんの許可を得て、@c649.私が鍵を開けました…@c999.」@r@z25.@r@z100.「@c649.私たち使用人全員は、常に一緒におりました@c999.」",
    "@z110.■第一の晩@z100.@r「@c649.僕と南條先生は、源次さまが死んでいることを確認しました@c999.」@r@z25.@r@z100.@z110.■第二の晩@z100.@r「@c649.部屋の中には、蔵臼さまと夏妃さまが倒れていました@c999.」@r@z25.@r@z100.「@c649.僕たち使用人全員は、それぞれが使用人全員のアリバイを証明できます@c999.」",
    "@z110.■第一の晩@z100.@r「@c649.食堂は施錠されていました。@c999.確かに食堂に鍵は付いていますが、普段、施錠することなどありません。ノックしたのですが返事はなく…」@r@z25.@r@z100.「@c649.マスターキーで扉を開ける@c999.と、中は恐ろしいことに……」@r@z25.@r@z100.@z110.■第二の晩@z100.@r「@c649.奥様の部屋は施錠されておりました！@c999.　食堂の時と同じです！」@r@z25.@r@z100.@z110.■第四の晩@z100.@r「二人が出掛けた時、私たちはゲストハウスの戸締りを確認するために色々とやっていました。だから、@c649.私たちにはみんな、アリバイがないのです…！@c999.」@r@z25.@r@z100.「犯人に悪用されるのを防ぐため、@c649.紗音さんの持ってたマスターキーは、その場で破壊しました@c999.」",
    "@z110.■第一の晩@z100.@r「@c649.そこに私が来たのです。@c999.万一のこともあるから、鍵を開けてみたらと勧めたのです」@r@z25.@r@z100.「@c649.絵羽さま、秀吉さま、留弗夫さま、霧江さま、楼座さま、そして源次さんの、計６人が血塗れになって倒れていたのでございます……@c999.」",
];

pub const PERSON_SOURCE_ADDRESSES: [usize; 17] = [
    0x15c6e5, 0x1605a4, 0x15f4ac, 0x162118, 0x162118, 0x162696, 0x162118, 0x162118, 0x15c877,
    0x162118, 0x15a041, 0x15b59c, 0x162118, 0x161806, 0x16002c, 0x15fa13, 0x15c21a,
];

// Native voice/speaker keys used by header setup0x94740, including empties.
pub const PERSON_KEYS: [&str; 17] = [
    "kla", "nat", "jes", "", "", "geo", "", "", "but", "", "mar", "nan", "", "sha", "kan", "goh",
    "kum",
];
