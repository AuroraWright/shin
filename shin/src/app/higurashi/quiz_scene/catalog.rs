//! Original immutable8x60 question tables and ten position-dependent specials.
//! Updated image SHA256:4273bbd17e89ced3d44e4d47adc4a234343526f31e8766287282c0b2a9c7395c.
use super::Question;
pub(super) const POOLS: [[Question; 60]; 8] = [
    [
        Question {
            voice: "VT_QL1_1001",
            texture: "qlv1_01",
            answers: ["5個", "4個", "3個", "12個"],
        },
        Question {
            voice: "VT_QL1_1002",
            texture: "qlv1_02",
            answers: ["豚骨ショウガ味", "醤油味", "塩味", "カレー"],
        },
        Question {
            voice: "VT_QL1_1003",
            texture: "qlv1_03",
            answers: ["右腕", "頭", "左腕", "明日"],
        },
        Question {
            voice: "VT_QL1_1004",
            texture: "qlv1_04",
            answers: ["29歳", "28歳", "25歳", "永遠の17歳"],
        },
        Question {
            voice: "VT_QL1_1005",
            texture: "qlv1_05",
            answers: ["祐子", "洋子", "路子", "優子"],
        },
        Question {
            voice: "VT_QL1_1006",
            texture: "qlv1_06",
            answers: ["沁子", "ミタ", "芳子", "幸子"],
        },
        Question {
            voice: "VT_QL1_1007",
            texture: "qlv1_07",
            answers: ["善郎", "弘", "義郎", "三郎"],
        },
        Question {
            voice: "VT_QL1_1008",
            texture: "qlv1_08",
            answers: ["義郎", "善郎", "弘", "三郎"],
        },
        Question {
            voice: "VT_QL1_1009",
            texture: "qlv1_09",
            answers: ["ファイターズ", "ライオンズ", "タイガース", "アフィリアーズ"],
        },
        Question {
            voice: "VT_QL1_1010",
            texture: "qlv1_10",
            answers: ["幸一", "進一", "剛", "秀樹"],
        },
        Question {
            voice: "VT_QL1_1011",
            texture: "qlv1_11",
            answers: ["カレー", "鯛焼き", "バニラアイス", "お米"],
        },
        Question {
            voice: "VT_QL1_1012",
            texture: "qlv1_12",
            answers: ["かぼちゃ", "ブロッコリー", "カリフラワー", "ロマネスコ"],
        },
        Question {
            voice: "VT_QL1_1013",
            texture: "qlv1_13",
            answers: ["夏美", "圭一", "入江", "陸"],
        },
        Question {
            voice: "VT_QL1_1014",
            texture: "qlv1_14",
            answers: ["8代目", "7代目", "3代目", "9代目"],
        },
        Question {
            voice: "VT_QL1_1015",
            texture: "qlv1_15",
            answers: [
                "白ヒゲのおっさん",
                "ゾウの子ども",
                "舌を出した女の子",
                "ピエロみたいな男",
            ],
        },
        Question {
            voice: "VT_QL1_1016",
            texture: "qlv1_16",
            answers: ["同人作家", "画家", "彫刻家", "遊び人"],
        },
        Question {
            voice: "VT_QL1_1017",
            texture: "qlv1_17",
            answers: ["レナ", "魅音", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL1_1018",
            texture: "qlv1_18",
            answers: ["沙都子", "魅音", "レナ", "梨花"],
        },
        Question {
            voice: "VT_QL1_1019",
            texture: "qlv1_19",
            answers: ["梨花", "魅音", "レナ", "沙都子"],
        },
        Question {
            voice: "VT_QL1_1020",
            texture: "qlv1_20",
            answers: ["沙都子", "魅音", "レナ", "梨花"],
        },
        Question {
            voice: "VT_QL1_1021",
            texture: "qlv1_21",
            answers: ["魅音", "レナ", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL1_1022",
            texture: "qlv1_22",
            answers: ["詩音", "魅音", "レナ", "沙都子"],
        },
        Question {
            voice: "VT_QL1_1023",
            texture: "qlv1_23",
            answers: ["傷害", "窃盗", "放火", "殺人"],
        },
        Question {
            voice: "VT_QL1_1024",
            texture: "qlv1_24",
            answers: ["明王", "帝王", "界王", "覇王"],
        },
        Question {
            voice: "VT_QL1_1025",
            texture: "qlv1_25",
            answers: ["合掌", "合唱", "合従", "楽笑"],
        },
        Question {
            voice: "VT_QL1_1026",
            texture: "qlv1_26",
            answers: ["綿", "罪", "臓物", "涙"],
        },
        Question {
            voice: "VT_QL1_1027",
            texture: "qlv1_27",
            answers: ["布団", "ニワトリ", "人", "国旗"],
        },
        Question {
            voice: "VT_QL1_1028",
            texture: "qlv1_28",
            answers: ["風呂場", "台所", "庭", "居間"],
        },
        Question {
            voice: "VT_QL1_1029",
            texture: "qlv1_29",
            answers: ["総合病院", "宝石店", "レストラン", "製薬会社"],
        },
        Question {
            voice: "VT_QL1_1030",
            texture: "qlv1_30",
            answers: ["佳織", "沙織", "伊織", "詩織"],
        },
        Question {
            voice: "VT_QL1_1031",
            texture: "qlv1_31",
            answers: ["松岡", "高屋敷", "安藤", "嶋"],
        },
        Question {
            voice: "VT_QL1_1032",
            texture: "qlv1_32",
            answers: ["モデルガン", "パチンコ", "麻雀", "トランプ"],
        },
        Question {
            voice: "VT_QL1_1033",
            texture: "qlv1_33",
            answers: ["ベーゴマ", "ゾンビ鬼", "大富豪", "犯人探し"],
        },
        Question {
            voice: "VT_QL1_1034",
            texture: "qlv1_34",
            answers: ["金属バット", "鉈", "斧", "スタンガン"],
        },
        Question {
            voice: "VT_QL1_1035",
            texture: "qlv1_35",
            answers: ["松井菜桜子", "前田愛", "水野理紗", "井上麻里奈"],
        },
        Question {
            voice: "VT_QL1_1036",
            texture: "qlv1_36",
            answers: ["猫殺し編", "村崩し編", "幸遠し編", "夢交し編"],
        },
        Question {
            voice: "VT_QL1_1037",
            texture: "qlv1_37",
            answers: ["2008年", "2007年", "2009年", "2010年"],
        },
        Question {
            voice: "VT_QL1_1038",
            texture: "qlv1_38",
            answers: ["彩音", "いとうかなこ", "Cyua", "諫山実生"],
        },
        Question {
            voice: "VT_QL1_1039",
            texture: "qlv1_39",
            answers: ["祟", "想", "螺", "絆"],
        },
        Question {
            voice: "VT_QL1_1040",
            texture: "qlv1_40",
            answers: ["いとうかなこ", "彩音", "Cyua", "諫山実生"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv1_41",
            answers: ["おきのみや", "こうみや", "きょうみや", "こうぐう"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv1_42",
            answers: ["ししぼね", "かこつ", "しかほね", "ろっこつ"],
        },
        Question {
            voice: "VT_QL0_1002",
            texture: "qlv1_43",
            answers: ["レナ", "魅音", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL0_1002",
            texture: "qlv1_44",
            answers: ["魅音", "レナ", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_45",
            answers: ["圭一", "悟史", "幸助", "誠"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_46",
            answers: ["レナ", "魅音", "梨花", "文乃"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_47",
            answers: ["魅音", "レナ", "沙都子", "伊豆湖"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_48",
            answers: ["沙都子", "梨花", "レナ", "ようこ"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_49",
            answers: ["梨花", "沙都子", "夏美", "むろみ"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_50",
            answers: ["夏美", "みちる", "真奈美", "マミ"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_51",
            answers: ["藤田", "熊田", "郷田", "花田"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv1_52",
            answers: ["リナ", "リサ", "ナサ", "サナリィ"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_53",
            answers: ["保志総一朗", "星総一郎", "保志総次郎", "瀬戸総一郎"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_54",
            answers: ["中原麻衣", "仲原舞", "中原真衣", "楯山麻衣"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_55",
            answers: [
                "ゆきのさつき",
                "ゆきのむつき",
                "きみざわさつき",
                "ゆきのかなめ",
            ],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_56",
            answers: ["かないみか", "かみなかい", "かみいなか", "かかみいな"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_57",
            answers: ["田村ゆかり", "田無ゆかり", "田村ゆりか", "村田ゆりか"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_58",
            answers: ["水橋かおり", "涼宮かおり", "荻上かおり", "巴かすみ"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_59",
            answers: ["宝亀克寿", "宝亀克久", "奉亀克寿", "ラル・ジャクソン"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv1_60",
            answers: ["氷上恭子", "氷上共子", "氷上響子", "シフォン・うさだ"],
        },
    ],
    [
        Question {
            voice: "VT_QL2_1001",
            texture: "qlv2_01",
            answers: ["警視庁", "警察庁", "東京都庁", "大江戸捜査網"],
        },
        Question {
            voice: "VT_QL2_1002",
            texture: "qlv2_02",
            answers: ["円美", "一美", "玉美", "丸美"],
        },
        Question {
            voice: "VT_QL2_1003",
            texture: "qlv2_03",
            answers: ["東京", "ソレント", "田舎", "無"],
        },
        Question {
            voice: "VT_QL2_1004",
            texture: "qlv2_04",
            answers: ["転落死", "病死", "中毒死", "窒息死"],
        },
        Question {
            voice: "VT_QL2_1005",
            texture: "qlv2_05",
            answers: ["喜一郎", "又一郎", "総一郎", "喜八"],
        },
        Question {
            voice: "VT_QL2_1006",
            texture: "qlv2_06",
            answers: ["缶詰", "レモン", "しいたけ", "漬物"],
        },
        Question {
            voice: "VT_QL2_1007",
            texture: "qlv2_07",
            answers: ["キャリア", "推薦組", "一般採用", "出向"],
        },
        Question {
            voice: "VT_QL2_1008",
            texture: "qlv2_08",
            answers: ["聖ルチーア", "聖マリア", "聖ナポリ", "聖セレナ"],
        },
        Question {
            voice: "VT_QL2_1009",
            texture: "qlv2_09",
            answers: ["サトさん", "ソトさん", "ミチさん", "スズさん"],
        },
        Question {
            voice: "VT_QL2_1010",
            texture: "qlv2_10",
            answers: ["美雪", "深雪", "美幸", "みゆき"],
        },
        Question {
            voice: "VT_QL2_1011",
            texture: "qlv2_11",
            answers: ["リンゴ", "絶食", "バナナ", "炭水化物"],
        },
        Question {
            voice: "VT_QL2_1012",
            texture: "qlv2_12",
            answers: ["社交ダンス", "農業", "盆栽", "ゲートボール"],
        },
        Question {
            voice: "VT_QL2_1013",
            texture: "qlv2_13",
            answers: ["第七室", "第五室", "第一室", "第零室"],
        },
        Question {
            voice: "VT_QL2_1014",
            texture: "qlv2_14",
            answers: ["3歳", "2歳", "5歳", "4歳"],
        },
        Question {
            voice: "VT_QL2_1015",
            texture: "qlv2_15",
            answers: ["山口", "長崎", "和歌山", "愛媛"],
        },
        Question {
            voice: "VT_QL2_1016",
            texture: "qlv2_16",
            answers: ["辰由", "辰吉", "辰良", "辰義"],
        },
        Question {
            voice: "VT_QL2_1017",
            texture: "qlv2_17",
            answers: ["刺青", "胸", "髪形", "頭の良さ"],
        },
        Question {
            voice: "VT_QL2_1018",
            texture: "qlv2_18",
            answers: ["勝也", "篤士", "拓央", "充"],
        },
        Question {
            voice: "VT_QL2_1019",
            texture: "qlv2_19",
            answers: ["フワラズ", "フラワズ", "プリホル", "ワラフズ"],
        },
        Question {
            voice: "VT_QL2_1020",
            texture: "qlv2_20",
            answers: [
                "最初に惚れる相手",
                "圭一の固有結界",
                "最後の対決方法",
                "羽入と詩音の有無",
            ],
        },
        Question {
            voice: "VT_QL2_1021",
            texture: "qlv2_21",
            answers: ["あおい", "正雄", "一馬", "芳子"],
        },
        Question {
            voice: "VT_QL2_1022",
            texture: "qlv2_22",
            answers: ["ファミレス", "おもちゃ屋", "図書館", "キャバクラ"],
        },
        Question {
            voice: "VT_QL2_1023",
            texture: "qlv2_23",
            answers: ["詩音", "鷹野", "富竹", "圭一"],
        },
        Question {
            voice: "VT_QL2_1024",
            texture: "qlv2_24",
            answers: ["水野理紗", "松井菜桜子", "前田愛", "井上麻里奈"],
        },
        Question {
            voice: "VT_QL2_1025",
            texture: "qlv2_25",
            answers: ["赤坂美雪", "田無美代子", "十和田八重", "南井まどか"],
        },
        Question {
            voice: "VT_QL2_1026",
            texture: "qlv2_26",
            answers: ["諫山実生", "いとうかなこ", "彩音", "Cyua"],
        },
        Question {
            voice: "VT_QL2_1027",
            texture: "qlv2_27",
            answers: ["you", "そらのむこう", "Friend", "escape"],
        },
        Question {
            voice: "VT_QL2_1028",
            texture: "qlv2_28",
            answers: ["dai", "doi", "bai", "boi"],
        },
        Question {
            voice: "VT_QL2_1029",
            texture: "qlv2_29",
            answers: ["鬼曝し編", "現壊し編", "心癒し編", "燕返し編"],
        },
        Question {
            voice: "VT_QL2_1030",
            texture: "qlv2_30",
            answers: ["ひデブ", "さンゴ", "トさカ", "たワバ"],
        },
        Question {
            voice: "VT_QL2_1031",
            texture: "qlv2_31",
            answers: ["現場監督", "北条夫妻", "古手夫妻", "北条玉枝"],
        },
        Question {
            voice: "VT_QL2_1032",
            texture: "qlv2_32",
            answers: ["北条夫妻", "現場監督", "古手夫妻", "北条玉枝"],
        },
        Question {
            voice: "VT_QL2_1033",
            texture: "qlv2_33",
            answers: ["古手夫妻", "現場監督", "北条夫妻", "北条玉枝"],
        },
        Question {
            voice: "VT_QL2_1034",
            texture: "qlv2_34",
            answers: ["北条玉枝", "現場監督", "北条夫妻", "古手夫妻"],
        },
        Question {
            voice: "VT_QL2_1035",
            texture: "qlv2_35",
            answers: ["2008年", "2007年", "2009年", "2010年"],
        },
        Question {
            voice: "VT_QL2_1036",
            texture: "qlv2_36",
            answers: ["カケラ遊び", "カケラ紡ぎ", "カケラ結び", "カケラ弾き"],
        },
        Question {
            voice: "VT_QL2_1037",
            texture: "qlv2_37",
            answers: ["時報", "空気", "フラグ", "通過点"],
        },
        Question {
            voice: "VT_QL2_1038",
            texture: "qlv2_38",
            answers: ["rato", "Rato", "RATO", "Lato"],
        },
        Question {
            voice: "VT_QL2_1039",
            texture: "qlv2_39",
            answers: ["想", "祟", "螺", "絆"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv2_40",
            answers: ["やごうち", "たにごうち", "たにこうち", "たにかわち"],
        },
        Question {
            voice: "VT_QL0_1002",
            texture: "qlv2_41",
            answers: ["沙都子", "レナ", "詩音", "梨花"],
        },
        Question {
            voice: "VT_QL0_1002",
            texture: "qlv2_42",
            answers: ["梨花", "夏美", "魅音", "詩音"],
        },
        Question {
            voice: "VT_QL0_1002",
            texture: "qlv2_43",
            answers: ["詩音", "魅音", "レナ", "夏美"],
        },
        Question {
            voice: "VT_QL0_1002",
            texture: "qlv2_44",
            answers: ["夏美", "レナ", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL0_1003",
            texture: "qlv2_45",
            answers: ["魅音", "レナ", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_46",
            answers: ["梨花", "沙都子", "美雪", "夏美"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_47",
            answers: ["赤坂", "戦人", "保坂", "空条"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_48",
            answers: ["詩音", "嘉音", "紗音", "明日音"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_49",
            answers: ["亀田", "田中", "烏養", "小林"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_50",
            answers: ["悟史", "鉄平", "聡志", "嘉音"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_51",
            answers: ["巴", "一美", "薙", "麻尋"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv2_52",
            answers: ["大石", "鎌霧", "磯野", "目暮"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_53",
            answers: ["田村ゆかり", "村田ゆかり", "田村ゆりか", "完売なのは"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_54",
            answers: ["小野大輔", "右代宮大輔", "小野清舟", "小野かおる"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_55",
            answers: [
                "ゆきのさつき",
                "ゆきのむつき",
                "きみざわさつき",
                "ゆきのかなめ",
            ],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_56",
            answers: ["田中一成", "田中三成", "道頓堀一成", "田中拳"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_57",
            answers: ["小林ゆう", "木林ゆう", "小林ゆゆ", "サシャ・葛城"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_58",
            answers: ["井ノ上奈々", "井上七", "川淵奈々", "仙堂奈々"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_59",
            answers: ["茶風林", "茶風鈴", "滝口林", "永沢秀治"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv2_60",
            answers: ["沢城みゆき", "沢城美雪", "沢代みゆき", "銅鑼野織乃楠"],
        },
    ],
    [
        Question {
            voice: "VT_QL3_1001",
            texture: "qlv3_01",
            answers: ["鹿嶋大", "水戸大", "日立大", "土浦大"],
        },
        Question {
            voice: "VT_QL3_1002",
            texture: "qlv3_02",
            answers: ["警視正", "警視", "警視長", "警部"],
        },
        Question {
            voice: "VT_QL3_1003",
            texture: "qlv3_03",
            answers: ["三郎", "善郎", "弘", "義郎"],
        },
        Question {
            voice: "VT_QL3_1004",
            texture: "qlv3_04",
            answers: ["走り高跳び", "マラソン", "やり投げ", "短距離走"],
        },
        Question {
            voice: "VT_QL3_1005",
            texture: "qlv3_05",
            answers: ["穀倉", "谷河内", "興宮", "天竺"],
        },
        Question {
            voice: "VT_QL3_1006",
            texture: "qlv3_06",
            answers: [
                "キッチンタイマー",
                "目覚まし時計",
                "フーコーの振り子",
                "形状記憶合金",
            ],
        },
        Question {
            voice: "VT_QL3_1007",
            texture: "qlv3_07",
            answers: ["高杉", "大杉", "出木杉", "千年杉"],
        },
        Question {
            voice: "VT_QL3_1008",
            texture: "qlv3_08",
            answers: ["正解はない", "キムチ", "ワサビ", "ハバネロ"],
        },
        Question {
            voice: "VT_QL3_1009",
            texture: "qlv3_09",
            answers: ["ぶどうジュース", "りんごジュース", "梅酒", "焼肉のたれ"],
        },
        Question {
            voice: "VT_QL3_1010",
            texture: "qlv3_10",
            answers: ["大公爵", "帝王", "神", "おっさん"],
        },
        Question {
            voice: "VT_QL3_1011",
            texture: "qlv3_11",
            answers: ["振鈴", "心霊", "神鈴", "神礼"],
        },
        Question {
            voice: "VT_QL3_1012",
            texture: "qlv3_12",
            answers: ["たま", "つま", "しま", "てま"],
        },
        Question {
            voice: "VT_QL3_1013",
            texture: "qlv3_13",
            answers: ["工", "匠", "巧", "卓"],
        },
        Question {
            voice: "VT_QL3_1014",
            texture: "qlv3_14",
            answers: ["オカルト", "スポーツ", "ＳＦ", "芸能"],
        },
        Question {
            voice: "VT_QL3_1015",
            texture: "qlv3_15",
            answers: ["赤坂", "大石", "富竹", "圭一"],
        },
        Question {
            voice: "VT_QL3_1016",
            texture: "qlv3_16",
            answers: ["三船", "渡", "加山", "石原"],
        },
        Question {
            voice: "VT_QL3_1017",
            texture: "qlv3_17",
            answers: ["サインペン", "泡だて器", "リモコン", "しゃもじ"],
        },
        Question {
            voice: "VT_QL3_1018",
            texture: "qlv3_18",
            answers: ["ブルー", "リトル", "レッド", "デス"],
        },
        Question {
            voice: "VT_QL3_1019",
            texture: "qlv3_19",
            answers: ["彼女", "競馬", "パチンコ", "宗教"],
        },
        Question {
            voice: "VT_QL3_1020",
            texture: "qlv3_20",
            answers: ["乙部", "芥川", "千秋", "真知子"],
        },
        Question {
            voice: "VT_QL3_1021",
            texture: "qlv3_21",
            answers: ["園芸部", "演劇部", "吹奏楽部", "文芸部"],
        },
        Question {
            voice: "VT_QL3_1022",
            texture: "qlv3_22",
            answers: ["澤村", "田中", "前田", "大谷"],
        },
        Question {
            voice: "VT_QL3_1023",
            texture: "qlv3_23",
            answers: ["蒐", "朱", "明", "愛"],
        },
        Question {
            voice: "VT_QL3_1024",
            texture: "qlv3_24",
            answers: [
                "喫茶店で立ち聞き",
                "預金が減った",
                "家具が増えた",
                "父のキャバクラ",
            ],
        },
        Question {
            voice: "VT_QL3_1025",
            texture: "qlv3_25",
            answers: ["デザイナー", "教師", "モデル", "アナウンサー"],
        },
        Question {
            voice: "VT_QL3_1026",
            texture: "qlv3_26",
            answers: ["バケモノ", "悪魔", "鬼", "宇宙人"],
        },
        Question {
            voice: "VT_QL3_1027",
            texture: "qlv3_27",
            answers: ["神", "考古学者", "医師", "政治家"],
        },
        Question {
            voice: "VT_QL3_1028",
            texture: "qlv3_28",
            answers: ["26名", "27名", "30名", "21名"],
        },
        Question {
            voice: "VT_QL3_1029",
            texture: "qlv3_29",
            answers: [
                "ルナティック",
                "オカルティック",
                "カオティック",
                "アクロバティック",
            ],
        },
        Question {
            voice: "VT_QL3_1030",
            texture: "qlv3_30",
            answers: ["大阪府警", "警視庁", "岐阜県警", "愛知県警"],
        },
        Question {
            voice: "VT_QL3_1031",
            texture: "qlv3_31",
            answers: ["Cyua", "いとうかなこ", "彩音", "諫山実生"],
        },
        Question {
            voice: "VT_QL3_1032",
            texture: "qlv3_32",
            answers: ["いとうかなこ", "彩音", "Cyua", "諫山実生"],
        },
        Question {
            voice: "VT_QL3_1033",
            texture: "qlv3_33",
            answers: ["escape", "そらのむこう", "you", "Friend"],
        },
        Question {
            voice: "VT_QL3_1034",
            texture: "qlv3_34",
            answers: ["ヤンデレ", "ツンデレ", "クーデレ", "極デレ"],
        },
        Question {
            voice: "VT_QL3_1035",
            texture: "qlv3_35",
            answers: ["キャラが増えた", "死人が多い", "バッドエンド", "年号が違う"],
        },
        Question {
            voice: "VT_QL3_1036",
            texture: "qlv3_36",
            answers: ["宮城季詩子", "中野麻衣", "中村慎介", "鴻野貴光"],
        },
        Question {
            voice: "VT_QL3_1037",
            texture: "qlv3_37",
            answers: ["2009年", "2007年", "2008年", "2010年"],
        },
        Question {
            voice: "VT_QL3_1038",
            texture: "qlv3_38",
            answers: ["吸血欲求", "疑心暗鬼", "喉が痒くなる", "幻覚が見える"],
        },
        Question {
            voice: "VT_QL3_1039",
            texture: "qlv3_39",
            answers: ["螺", "祟", "想", "絆"],
        },
        Question {
            voice: "VT_QL3_1040",
            texture: "qlv3_40",
            answers: ["ゆきのさつき", "保志総一朗", "中原麻衣", "田村ゆかり"],
        },
        Question {
            voice: "VT_QL3_1041",
            texture: "qlv3_41",
            answers: ["2007年", "2006年", "2008年", "2009年"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv3_42",
            answers: ["ごぐら", "こくそう", "こくら", "ごくら"],
        },
        Question {
            voice: "VT_QL0_1007",
            texture: "qlv3_43",
            answers: ["おこのぎ", "こひぎ", "ここのぎ", "ここぎ"],
        },
        Question {
            voice: "VT_QL0_1003",
            texture: "qlv3_44",
            answers: ["レナ", "魅音", "沙都子", "梨花"],
        },
        Question {
            voice: "VT_QL0_1003",
            texture: "qlv3_45",
            answers: ["沙都子", "レナ", "魅音", "梨花"],
        },
        Question {
            voice: "VT_QL0_1003",
            texture: "qlv3_46",
            answers: ["梨花", "レナ", "魅音", "沙都子"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_47",
            answers: ["乙部", "甲部", "乙吉", "光希"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_48",
            answers: ["八重", "二重", "一重", "沙英"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_49",
            answers: ["渚", "薫", "慎", "梢"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_50",
            answers: ["塚田", "山田", "有田", "満田"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_51",
            answers: ["まどか", "ほむら", "きょうこ", "さやか"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_52",
            answers: ["入江", "大石", "鷹野", "ＪＪ・リックス"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv3_53",
            answers: ["知恵", "巴", "まどか", "茜"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_54",
            answers: ["宮田幸季", "田宮幸季", "宮田光貴", "不二咲照文"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_55",
            answers: ["新谷良子", "新谷涼子", "仁井谷良子", "ミルフィ・桜木"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_56",
            answers: ["仁後真耶子", "二五真耶子", "人御摩耶子", "リリカ・高槻"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_57",
            answers: ["中村悠一", "仲村悠一", "中村裕一", "リカルド・早乙女"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_58",
            answers: ["下田麻美", "志茂田麻美", "下田麻実", "亜美真美"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_59",
            answers: ["関俊彦", "石俊彦", "関利彦", "加賀桃太郎"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv3_60",
            answers: ["折笠富美子", "織笠富美子", "折笠富子", "シエル・南野"],
        },
    ],
    [
        Question {
            voice: "VT_QL4_1001",
            texture: "qlv4_01",
            answers: ["1970年", "1971年", "1972年", "1969年"],
        },
        Question {
            voice: "VT_QL4_1002",
            texture: "qlv4_02",
            answers: ["カーレース", "盆栽", "射撃", "読書"],
        },
        Question {
            voice: "VT_QL4_1003",
            texture: "qlv4_03",
            answers: ["レストラン", "花屋", "不動産屋", "料亭"],
        },
        Question {
            voice: "VT_QL4_1004",
            texture: "qlv4_04",
            answers: ["児島", "山下", "駒鳥", "田淵"],
        },
        Question {
            voice: "VT_QL4_1005",
            texture: "qlv4_05",
            answers: ["滝口", "山口", "澤村", "種田"],
        },
        Question {
            voice: "VT_QL4_1006",
            texture: "qlv4_06",
            answers: ["弘", "善郎", "義郎", "三郎"],
        },
        Question {
            voice: "VT_QL4_1007",
            texture: "qlv4_07",
            answers: ["柳桜", "流桜", "桜桃", "龍桜"],
        },
        Question {
            voice: "VT_QL4_1008",
            texture: "qlv4_08",
            answers: ["来花", "桜花", "梨花", "風花"],
        },
        Question {
            voice: "VT_QL4_1009",
            texture: "qlv4_09",
            answers: ["山本", "沖田", "川上", "小早川"],
        },
        Question {
            voice: "VT_QL4_1010",
            texture: "qlv4_10",
            answers: ["イェアソムール", "シーツェン", "リューン", "ジェダ"],
        },
        Question {
            voice: "VT_QL4_1011",
            texture: "qlv4_11",
            answers: ["田無", "吉祥寺", "中野", "三鷹"],
        },
        Question {
            voice: "VT_QL4_1012",
            texture: "qlv4_12",
            answers: ["射撃", "タックル", "盗撮", "空気を読む"],
        },
        Question {
            voice: "VT_QL4_1013",
            texture: "qlv4_13",
            answers: ["警視", "警視正", "警部補", "警部"],
        },
        Question {
            voice: "VT_QL4_1014",
            texture: "qlv4_14",
            answers: ["保険証", "目撃証言", "DNA鑑定", "プロファイル"],
        },
        Question {
            voice: "VT_QL4_1015",
            texture: "qlv4_15",
            answers: ["警視庁", "警察庁", "茨城県警", "愛知県警"],
        },
        Question {
            voice: "VT_QL4_1016",
            texture: "qlv4_16",
            answers: ["土建職人", "画家", "医師", "教師"],
        },
        Question {
            voice: "VT_QL4_1017",
            texture: "qlv4_17",
            answers: ["精神外科", "内科", "小児科", "眼科"],
        },
        Question {
            voice: "VT_QL4_1018",
            texture: "qlv4_18",
            answers: ["鬼姫", "歌姫", "舞姫", "鬼婆"],
        },
        Question {
            voice: "VT_QL4_1019",
            texture: "qlv4_19",
            answers: [
                "ジュネーブ",
                "リオデジャネイロ",
                "ブエノスアイレス",
                "ブラジリア",
            ],
        },
        Question {
            voice: "VT_QL4_1020",
            texture: "qlv4_20",
            answers: ["ウナギ", "ピラニア", "トビウオ", "ペンギン"],
        },
        Question {
            voice: "VT_QL4_1021",
            texture: "qlv4_21",
            answers: [
                "立ったまま気絶",
                "鼻血吹き出す",
                "穴を掘って埋まる",
                "逃亡して行方不明",
            ],
        },
        Question {
            voice: "VT_QL4_1022",
            texture: "qlv4_22",
            answers: [
                "鼻血吹き出す",
                "立ったまま気絶",
                "穴を掘って埋まる",
                "逃亡して行方不明",
            ],
        },
        Question {
            voice: "VT_QL4_1023",
            texture: "qlv4_23",
            answers: ["森", "野村", "渡邉", "舞沢"],
        },
        Question {
            voice: "VT_QL4_1024",
            texture: "qlv4_24",
            answers: ["反町", "巴", "山沖", "前野"],
        },
        Question {
            voice: "VT_QL4_1025",
            texture: "qlv4_25",
            answers: ["射撃", "格闘技", "諜報", "麻雀"],
        },
        Question {
            voice: "VT_QL4_1026",
            texture: "qlv4_26",
            answers: ["魔央", "真緒", "真央", "真魚"],
        },
        Question {
            voice: "VT_QL4_1027",
            texture: "qlv4_27",
            answers: ["旗", "コイン", "切手", "キャップ"],
        },
        Question {
            voice: "VT_QL4_1028",
            texture: "qlv4_28",
            answers: ["一二三", "十三", "一八", "二三也"],
        },
        Question {
            voice: "VT_QL4_1029",
            texture: "qlv4_29",
            answers: ["ミント", "アイス", "ナイフ", "アフロ"],
        },
        Question {
            voice: "VT_QL4_1030",
            texture: "qlv4_30",
            answers: [
                "後ろに下がる",
                "真ん中を通る",
                "真上にジャンプ",
                "その場を動かない",
            ],
        },
        Question {
            voice: "VT_QL4_1031",
            texture: "qlv4_31",
            answers: ["座敷わらし", "コロポックル", "キジムナー", "ぬらりひょん"],
        },
        Question {
            voice: "VT_QL4_1032",
            texture: "qlv4_32",
            answers: ["広報室長", "刑事課長", "公安課長", "署長"],
        },
        Question {
            voice: "VT_QL4_1033",
            texture: "qlv4_33",
            answers: [
                "科学警察研究所",
                "科学捜査研究所",
                "麻薬取締官事務所",
                "東京地方検察庁",
            ],
        },
        Question {
            voice: "VT_QL4_1034",
            texture: "qlv4_34",
            answers: [
                "東京地方検察庁",
                "科学捜査研究所",
                "科学警察研究所",
                "麻薬取締官事務所",
            ],
        },
        Question {
            voice: "VT_QL4_1035",
            texture: "qlv4_35",
            answers: ["前田愛", "松井菜桜子", "水野理紗", "井上麻里奈"],
        },
        Question {
            voice: "VT_QL4_1036",
            texture: "qlv4_36",
            answers: ["井上麻里奈", "松井菜桜子", "前田愛", "水野理紗"],
        },
        Question {
            voice: "VT_QL4_1037",
            texture: "qlv4_37",
            answers: ["鷹野三四", "入江京介", "富竹ジロウ", "大石蔵人"],
        },
        Question {
            voice: "VT_QL4_1038",
            texture: "qlv4_38",
            answers: ["反町美雪", "竜宮レナ", "羽入", "園崎魅音"],
        },
        Question {
            voice: "VT_QL4_1039",
            texture: "qlv4_39",
            answers: ["黒い戦闘服", "白い宇宙服", "実は裸", "赤い羽衣"],
        },
        Question {
            voice: "VT_QL4_1040",
            texture: "qlv4_40",
            answers: ["熊", "狼", "猪", "象"],
        },
        Question {
            voice: "VT_QL4_1041",
            texture: "qlv4_41",
            answers: ["そらのむこう", "you", "Friend", "escape"],
        },
        Question {
            voice: "VT_QL4_1042",
            texture: "qlv4_42",
            answers: ["カレー", "肉じゃが", "オムライス", "シチュー"],
        },
        Question {
            voice: "VT_QL4_1043",
            texture: "qlv4_43",
            answers: ["徹甲弾", "焼夷弾", "核弾頭", "爆裂弾"],
        },
        Question {
            voice: "VT_QL4_1044",
            texture: "qlv4_44",
            answers: ["中村慎介", "中野麻衣", "宮城季詩子", "鴻野貴光"],
        },
        Question {
            voice: "VT_QL4_1045",
            texture: "qlv4_45",
            answers: ["31歳", "26歳", "40歳", "28歳"],
        },
        Question {
            voice: "VT_QL4_1046",
            texture: "qlv4_46",
            answers: ["絆", "祟", "想", "螺"],
        },
        Question {
            voice: "VT_QL4_1047",
            texture: "qlv4_47",
            answers: ["2010年", "2007年", "2008年", "2009年"],
        },
        Question {
            voice: "VT_QL4_1048",
            texture: "qlv4_48",
            answers: ["いとうかなこ", "彩音", "Cyua", "諫山実生"],
        },
        Question {
            voice: "VT_QL4_1049",
            texture: "qlv4_49",
            answers: ["彩音", "いとうかなこ", "Cyua", "諫山実生"],
        },
        Question {
            voice: "VT_QL4_1050",
            texture: "qlv4_50",
            answers: ["Friend", "そらのむこう", "you", "escape"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv4_51",
            answers: ["鷹野", "鷹埜", "高野", "ラムダデルタ"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv4_52",
            answers: ["イェアソムール", "尾八白様", "アウアウローラ", "ジェダ"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv4_53",
            answers: ["お魎", "お菊", "お梅", "お京"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv4_54",
            answers: ["桜花", "桃花", "彩花", "梨花"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv4_55",
            answers: ["雲雀13", "鶯13", "燕13", "天草十二"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv4_56",
            answers: ["佐藤利奈", "左藤利奈", "佐藤縁寿", "レールガン・御坂"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv4_57",
            answers: ["間島淳司", "真島淳司", "間島純次", "プリニー・稲穂"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv4_58",
            answers: ["大浦冬華", "田無三四", "田無美代子", "メリル・田無"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv4_59",
            answers: ["若本規夫", "若本ノリ夫", "若元紀夫", "穴子輝"],
        },
        Question {
            voice: "VT_QL0_1005",
            texture: "qlv4_60",
            answers: ["関智一", "関友一", "関俊一", "ドモン・骨川"],
        },
    ],
    [
        Question {
            voice: "VT_QL5_1001",
            texture: "qlv5_01",
            answers: ["公由真由美", "公由春子", "公由冬司", "公由アキ"],
        },
        Question {
            voice: "VT_QL5_1002",
            texture: "qlv5_02",
            answers: ["牧野沁子", "染シミ子", "牧野アンナ", "熊沢チヨ"],
        },
        Question {
            voice: "VT_QL5_1003",
            texture: "qlv5_03",
            answers: ["嘘だッ！", "嘘だっちゃ！", "嘘だぁ！！！", "嫌だッ！"],
        },
        Question {
            voice: "VT_QL5_1004",
            texture: "qlv5_04",
            answers: ["亀田", "辰吉", "竹原", "内藤"],
        },
        Question {
            voice: "VT_QL5_1005",
            texture: "qlv5_05",
            answers: ["散弾銃", "機関銃", "光線銃", "水鉄砲"],
        },
        Question {
            voice: "VT_QL5_1006",
            texture: "qlv5_06",
            answers: ["埼玉県", "茨城県", "岐阜県", "沖縄県"],
        },
        Question {
            voice: "VT_QL5_1007",
            texture: "qlv5_07",
            answers: ["大高", "織田", "大石", "熊谷"],
        },
        Question {
            voice: "VT_QL5_1008",
            texture: "qlv5_08",
            answers: ["稲葉友", "吉沢亮", "赤楚衛二", "瀬戸利樹"],
        },
        Question {
            voice: "VT_QL5_1009",
            texture: "qlv5_09",
            answers: ["4話", "3話", "12話", "11話"],
        },
        Question {
            voice: "VT_QL5_1010",
            texture: "qlv5_10",
            answers: ["NGT48", "AKB48", "乃木坂46", "SKE48"],
        },
        Question {
            voice: "VT_QL5_1011",
            texture: "qlv5_11",
            answers: [
                "ブレイクアウト！",
                "アウトブレイク！",
                "ブレイクスルー！",
                "が鳴き止まない",
            ],
        },
        Question {
            voice: "VT_QL5_1012",
            texture: "qlv5_12",
            answers: ["散", "礼", "祭", "雀"],
        },
        Question {
            voice: "VT_QL5_1013",
            texture: "qlv5_13",
            answers: ["岐阜", "富山", "長野", "福井"],
        },
        Question {
            voice: "VT_QL5_1014",
            texture: "qlv5_14",
            answers: ["礼奈", "玲奈", "麗奈", "れな"],
        },
        Question {
            voice: "VT_QL5_1015",
            texture: "qlv5_15",
            answers: ["葉っぱ", "たんぽぽ", "ジャーキー", "歯ブラシ"],
        },
        Question {
            voice: "VT_QL5_1016",
            texture: "qlv5_16",
            answers: ["眼", "鉈", "おはぎ", "ひぐ夫"],
        },
        Question {
            voice: "VT_QL5_1017",
            texture: "qlv5_17",
            answers: ["ひぐ夫", "ひぐしげ", "しげっるチー", "オヤシロリカ"],
        },
        Question {
            voice: "VT_QL5_1018",
            texture: "qlv5_18",
            answers: ["口先", "妄想", "ハッタリ", "漆黒"],
        },
        Question {
            voice: "VT_QL5_1019",
            texture: "qlv5_19",
            answers: ["廃車", "富竹の背中", "ケンタ君人形", "銭湯の煙突"],
        },
        Question {
            voice: "VT_QL5_1020",
            texture: "qlv5_20",
            answers: ["椅子", "金属バット", "スタンガン", "日本刀"],
        },
        Question {
            voice: "VT_QL5_1021",
            texture: "qlv5_21",
            answers: ["10円玉", "テレホンカード", "サイフ", "金塊"],
        },
        Question {
            voice: "VT_QL5_1022",
            texture: "qlv5_22",
            answers: ["抜け穴", "黄金", "宇宙船", "テスト答案"],
        },
        Question {
            voice: "VT_QL5_1023",
            texture: "qlv5_23",
            answers: ["鬼ケ淵村", "清見村", "荘川村", "白川村"],
        },
        Question {
            voice: "VT_QL5_1024",
            texture: "qlv5_24",
            answers: ["100年以上", "17年", "1000年", "28年"],
        },
        Question {
            voice: "VT_QL5_1025",
            texture: "qlv5_25",
            answers: ["クラウド", "ヴァン", "ティーダ", "ノクティス"],
        },
        Question {
            voice: "VT_QL5_1026",
            texture: "qlv5_26",
            answers: ["二尉", "三佐", "軍曹", "曹長"],
        },
        Question {
            voice: "VT_QL5_1027",
            texture: "qlv5_27",
            answers: [
                "エンジェルモート",
                "ハンナミラーズ",
                "びっくりモンキー",
                "シルバーパロット",
            ],
        },
        Question {
            voice: "VT_QL5_1028",
            texture: "qlv5_28",
            answers: ["義郎", "泰夫", "啓一", "兵吾"],
        },
        Question {
            voice: "VT_QL5_1029",
            texture: "qlv5_29",
            answers: ["むぅ…", "うりゅー", "はにゅん", "わにゃん"],
        },
        Question {
            voice: "VT_QL5_1030",
            texture: "qlv5_30",
            answers: ["ボールペン", "耳かき", "タッチペン", "充電器"],
        },
        Question {
            voice: "VT_QL5_1031",
            texture: "qlv5_31",
            answers: ["you", "your", "奈落の花", "漆黒の祭壇"],
        },
        Question {
            voice: "VT_QL5_1032",
            texture: "qlv5_32",
            answers: ["ガソリン", "除草剤", "金塊", "まきびし"],
        },
        Question {
            voice: "VT_QL5_1033",
            texture: "qlv5_33",
            answers: ["祝", "誓", "叫", "怪"],
        },
        Question {
            voice: "VT_QL5_1034",
            texture: "qlv5_34",
            answers: ["クラウド", "オリバー", "ウェイン", "チャールズ"],
        },
        Question {
            voice: "VT_QL5_1035",
            texture: "qlv5_35",
            answers: ["トミー", "サイラス", "ケイレブ", "リチャード"],
        },
        Question {
            voice: "VT_QL5_1036",
            texture: "qlv5_36",
            answers: ["イリー", "ジャンヌ", "ツェル", "ニーナ"],
        },
        Question {
            voice: "VT_QL5_1037",
            texture: "qlv5_37",
            answers: ["Ｋ", "アラン", "キース", "ガブリエル"],
        },
        Question {
            voice: "VT_QL5_1038",
            texture: "qlv5_38",
            answers: ["ブラザー", "ドラゴン", "キャット", "ファイター"],
        },
        Question {
            voice: "VT_QL5_1039",
            texture: "qlv5_39",
            answers: ["カレー", "ミートソース", "麻婆豆腐", "あったか寄せ鍋"],
        },
        Question {
            voice: "VT_QL5_1040",
            texture: "qlv5_40",
            answers: ["ともぞ", "緋賀ゆかり", "小池ノクト", "和遥キナ"],
        },
        Question {
            voice: "VT_QL5_1041",
            texture: "qlv5_41",
            answers: ["出題編", "前編", "ビギニング", "上巻"],
        },
        Question {
            voice: "VT_QL5_1042",
            texture: "qlv5_42",
            answers: ["解答編", "後編", "ラストシーズン", "下巻"],
        },
        Question {
            voice: "VT_QL5_1043",
            texture: "qlv5_43",
            answers: ["奏", "楽", "弦", "音"],
        },
        Question {
            voice: "VT_QL5_1044",
            texture: "qlv5_44",
            answers: ["雛見沢停留所", "心癒し編", "羞晒し編", "現壊し編"],
        },
        Question {
            voice: "VT_QL5_1044",
            texture: "qlv5_45",
            answers: ["アウトブレイク", "君恋し編", "語咄し編", "村崩し編"],
        },
        Question {
            voice: "VT_QL5_1044",
            texture: "qlv5_46",
            answers: ["神姦し編", "虚仮威し編", "幸探し編", "夢交し編"],
        },
        Question {
            voice: "VT_QL5_1044",
            texture: "qlv5_47",
            answers: ["罰恋し編", "厄醒し編", "夜更し編", "猫殺し編"],
        },
        Question {
            voice: "VT_QL5_1045",
            texture: "qlv5_48",
            answers: ["ＳＲＰＧ", "アドベンチャー", "麻雀", "対戦アクション"],
        },
        Question {
            voice: "VT_QL5_1046",
            texture: "qlv5_49",
            answers: ["症候群", "吊り橋効果", "病", "ショック"],
        },
        Question {
            voice: "VT_QL5_1047",
            texture: "qlv5_50",
            answers: ["松澤由美", "片霧烈火", "島みやえい子", "anNina"],
        },
        Question {
            voice: "VT_QL5_1048",
            texture: "qlv5_51",
            answers: ["副作用", "虚言", "妄想", "輪廻"],
        },
        Question {
            voice: "VT_QL5_1049",
            texture: "qlv5_52",
            answers: ["運気による変化", "疑心暗鬼", "平行世界", "信念を貫くこと"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv5_53",
            answers: ["けんこう", "こうよう", "こうふん", "のきあがり"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv5_54",
            answers: ["しかん", "しゆる", "ゆるゆる", "たるむ"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv5_55",
            answers: ["じゅうりん", "とうしゅう", "じゅうそく", "そくそく"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv5_56",
            answers: ["荒川龍ノ介", "荒川龍一", "畠山一馬", "黒澤工"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv5_57",
            answers: ["采", "彩", "舟", "稲"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv5_58",
            answers: ["ウネ", "フネ", "イネ", "クネ"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv5_59",
            answers: ["田村媛命", "村田姫", "田村みこと", "田村姫命"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv5_60",
            answers: ["荒川", "小日向", "右代宮", "前原"],
        },
    ],
    [
        Question {
            voice: "VT_QL6_1001",
            texture: "qlv6_01",
            answers: ["園崎組事務所", "入江診療所", "鬼ヶ淵沼", "古手神社"],
        },
        Question {
            voice: "VT_QL6_1002",
            texture: "qlv6_02",
            answers: ["駐輪禁止", "駐車禁止", "一時停止", "進入禁止"],
        },
        Question {
            voice: "VT_QL6_1003",
            texture: "qlv6_03",
            answers: ["コンプエース", "ヤングエース", "電撃大王", "ヤングガンガン"],
        },
        Question {
            voice: "VT_QL6_1004",
            texture: "qlv6_04",
            answers: [
                "ビッグガンガン",
                "ヤングガンガン",
                "ガンガンJOKER",
                "Gファンタジー",
            ],
        },
        Question {
            voice: "VT_QL6_1005",
            texture: "qlv6_05",
            answers: ["上間江望", "神代海凪", "神園しのぶ", "桐原歩鳥"],
        },
        Question {
            voice: "VT_QL6_1006",
            texture: "qlv6_06",
            answers: ["鉄平", "入江", "羽入", "采"],
        },
        Question {
            voice: "VT_QL6_1007",
            texture: "qlv6_07",
            answers: ["片霧烈火", "相原優気", "天弄御魂", "篝"],
        },
        Question {
            voice: "VT_QL6_1008",
            texture: "qlv6_08",
            answers: ["対象a", "対象r", "魔王k", "検体s"],
        },
        Question {
            voice: "VT_QL6_1009",
            texture: "qlv6_09",
            answers: ["cry", "laugh", "die", "sleep"],
        },
        Question {
            voice: "VT_QL6_1010",
            texture: "qlv6_10",
            answers: ["園崎お魎", "園崎詩音", "園崎茜", "園崎魅音"],
        },
        Question {
            voice: "VT_QL6_1011",
            texture: "qlv6_11",
            answers: ["海水パンツ", "メガホン", "バット", "カーテンのシャー"],
        },
        Question {
            voice: "VT_QL6_1012",
            texture: "qlv6_12",
            answers: ["クール", "ホット", "シット", "すったらん"],
        },
        Question {
            voice: "VT_QL6_1013",
            texture: "qlv6_13",
            answers: ["前原圭一", "前原Ｋ", "前原佳一", "北条悟史"],
        },
        Question {
            voice: "VT_QL6_1014",
            texture: "qlv6_14",
            answers: ["竜宮レナ", "龍宮レナ", "竜宮０７", "竜宮れな"],
        },
        Question {
            voice: "VT_QL6_1015",
            texture: "qlv6_15",
            answers: ["北条沙都子", "北城沙都子", "北条聡子", "北條紗都子"],
        },
        Question {
            voice: "VT_QL6_1016",
            texture: "qlv6_16",
            answers: ["古手梨花", "古手利花", "古出梨花", "フレデリカ"],
        },
        Question {
            voice: "VT_QL6_1017",
            texture: "qlv6_17",
            answers: ["古手羽入", "古出羽生", "古手ハイリューン", "古手羽生"],
        },
        Question {
            voice: "VT_QL6_1018",
            texture: "qlv6_18",
            answers: ["潜水", "戦略", "戦術", "戦闘"],
        },
        Question {
            voice: "VT_QL6_1019",
            texture: "qlv6_19",
            answers: ["草津の湯", "イケメン", "少女漫画", "手料理"],
        },
        Question {
            voice: "VT_QL6_1020",
            texture: "qlv6_20",
            answers: ["煌", "粋", "絆", "拡"],
        },
        Question {
            voice: "VT_QL6_1021",
            texture: "qlv6_21",
            answers: ["ぬいぐるみ", "自転車", "ゲーム機", "ランドセル"],
        },
        Question {
            voice: "VT_QL6_1022",
            texture: "qlv6_22",
            answers: [
                "カレー独身貴族",
                "パン屋の小娘",
                "知恵サマー",
                "インドに帰れ",
            ],
        },
        Question {
            voice: "VT_QL6_1023",
            texture: "qlv6_23",
            answers: ["鬼曝し編アフター", "裏返し編", "現壊し編", "燕返し編"],
        },
        Question {
            voice: "VT_QL6_1024",
            texture: "qlv6_24",
            answers: ["瑞穂", "ゆかり", "シスターマリア", "咲"],
        },
        Question {
            voice: "VT_QL6_1025",
            texture: "qlv6_25",
            answers: ["大石", "赤坂", "巴", "反町"],
        },
        Question {
            voice: "VT_QL6_1026",
            texture: "qlv6_26",
            answers: [
                "Gファンタジー",
                "ヤングガンガン",
                "コンプエース",
                "ビッグガンガン",
            ],
        },
        Question {
            voice: "VT_QL6_1027",
            texture: "qlv6_27",
            answers: ["さくたろう", "メイド服", "巫女", "ねこ耳スク水"],
        },
        Question {
            voice: "VT_QL6_1028",
            texture: "qlv6_28",
            answers: ["星海社文庫", "スニーカー文庫", "電撃文庫", "ＧＡ文庫"],
        },
        Question {
            voice: "VT_QL6_1029",
            texture: "qlv6_29",
            answers: [
                "お漬物ありがとう",
                "カレーありがとう",
                "おでんありがとう",
                "煮物ありがとう",
            ],
        },
        Question {
            voice: "VT_QL6_1030",
            texture: "qlv6_30",
            answers: ["夏美", "圭一", "レナ", "魅音"],
        },
        Question {
            voice: "VT_QL6_1031",
            texture: "qlv6_31",
            answers: ["垣内", "興宮", "穀倉", "高津戸"],
        },
        Question {
            voice: "VT_QL6_1032",
            texture: "qlv6_32",
            answers: ["デザートフェスタ", "苺祭", "肉フェス", "すったて鍋合戦"],
        },
        Question {
            voice: "VT_QL6_1033",
            texture: "qlv6_33",
            answers: ["ボードゲーム", "テレビゲーム", "ＬＳＩゲーム", "知育ゲーム"],
        },
        Question {
            voice: "VT_QL6_1034",
            texture: "qlv6_34",
            answers: ["すずり", "バケツ", "黒板消し", "濡れた雑巾"],
        },
        Question {
            voice: "VT_QL6_1035",
            texture: "qlv6_35",
            answers: ["デコピン", "しっぺ", "頭突き", "くすぐり"],
        },
        Question {
            voice: "VT_QL6_1036",
            texture: "qlv6_36",
            answers: ["カメラマン", "フリーター", "建設省職員", "時報役"],
        },
        Question {
            voice: "VT_QL6_1037",
            texture: "qlv6_37",
            answers: ["こんにちは", "鷹野よ", "こんばんは", "あら、詩音ちゃん"],
        },
        Question {
            voice: "VT_QL6_1038",
            texture: "qlv6_38",
            answers: ["エンジェルモート", "セブンスマート", "園崎家", "古手神社"],
        },
        Question {
            voice: "VT_QL6_1039",
            texture: "qlv6_39",
            answers: ["圭一の父", "レナ", "魅音", "富竹"],
        },
        Question {
            voice: "VT_QL6_1040",
            texture: "qlv6_40",
            answers: ["制服", "Ｖ．Ｇ．", "スイーツ", "ランチ"],
        },
        Question {
            voice: "VT_QL6_1041",
            texture: "qlv6_41",
            answers: ["父さん", "母さん", "息子", "女王様"],
        },
        Question {
            voice: "VT_QL6_1042",
            texture: "qlv6_42",
            answers: ["リナ", "リサ", "リタ", "メイシェ"],
        },
        Question {
            voice: "VT_QL6_1043",
            texture: "qlv6_43",
            answers: [
                "ビーフカレー",
                "ポークカレー",
                "カツカレー",
                "シーフードカレー",
            ],
        },
        Question {
            voice: "VT_QL6_1044",
            texture: "qlv6_44",
            answers: ["ジャガバター", "たこ焼き", "かき氷", "綿アメ"],
        },
        Question {
            voice: "VT_QL6_1045",
            texture: "qlv6_45",
            answers: ["オットセイ", "猫", "山羊", "イルカ"],
        },
        Question {
            voice: "VT_QL6_1046",
            texture: "qlv6_46",
            answers: ["名誉市民", "特別メンバー", "一日市民", "特待生"],
        },
        Question {
            voice: "VT_QL6_1047",
            texture: "qlv6_47",
            answers: ["クマ", "ライオン", "オットセイ", "ペンギン"],
        },
        Question {
            voice: "VT_QL6_1048",
            texture: "qlv6_48",
            answers: ["綿", "餅", "水飴", "てるてる坊主"],
        },
        Question {
            voice: "VT_QL6_1049",
            texture: "qlv6_49",
            answers: ["神主", "行司", "住職", "小料理屋"],
        },
        Question {
            voice: "VT_QL6_1050",
            texture: "qlv6_50",
            answers: [
                "太めの注射針",
                "苦くないお薬",
                "痛くない注射針",
                "熱いほうじ茶",
            ],
        },
        Question {
            voice: "VT_QL6_1051",
            texture: "qlv6_51",
            answers: ["古手神社", "園崎家", "雛見沢分校", "公由家"],
        },
        Question {
            voice: "VT_QL6_1052",
            texture: "qlv6_52",
            answers: ["梨花", "富竹", "鷹野", "詩音"],
        },
        Question {
            voice: "VT_QL6_1053",
            texture: "qlv6_53",
            answers: [
                "ジャンクフード",
                "スローフード",
                "ファーストフード",
                "ソウルフード",
            ],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv6_54",
            answers: ["かしゃく", "かせき", "かっせき", "どなりぜめ"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv6_55",
            answers: ["じゅすい", "にゅうすい", "いりみず", "いりすい"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv6_56",
            answers: ["じゅうたん", "じゅうだん", "けむしろ", "せんい"],
        },
        Question {
            voice: "VT_QL0_1006",
            texture: "qlv6_57",
            answers: ["ひれつかん", "くず", "ひれつおとこ", "ひじゃくかん"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv6_58",
            answers: ["魅音", "詩音", "美音", "海音"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv6_59",
            answers: ["梨花", "利香", "理香", "花梨"],
        },
        Question {
            voice: "VT_QL0_1004",
            texture: "qlv6_60",
            answers: ["吉村", "吉岡", "有村", "荒川"],
        },
    ],
    [
        Question {
            voice: "VT_QL7_1001",
            texture: "qlv7_01",
            answers: [
                "デイキャンプ",
                "ブートキャンプ",
                "ゆるキャン",
                "オートキャンプ",
            ],
        },
        Question {
            voice: "VT_QL7_1002",
            texture: "qlv7_02",
            answers: ["がおー", "シャアアア", "にょろにょろ", "ゲロゲロ"],
        },
        Question {
            voice: "VT_QL7_1003",
            texture: "qlv7_03",
            answers: ["ハンバーグ", "ロースカツ", "エビフライ", "ウスターソース"],
        },
        Question {
            voice: "VT_QL7_1004",
            texture: "qlv7_04",
            answers: ["100点", "10点", "0点", "及第点"],
        },
        Question {
            voice: "VT_QL7_1005",
            texture: "qlv7_05",
            answers: ["手作り弁当", "カップ麺", "おはぎ", "ホールケーキ"],
        },
        Question {
            voice: "VT_QL7_1006",
            texture: "qlv7_06",
            answers: ["校長", "知恵", "校医", "日直"],
        },
        Question {
            voice: "VT_QL7_1007",
            texture: "qlv7_07",
            answers: ["青", "黄", "黒", "茶"],
        },
        Question {
            voice: "VT_QL7_1008",
            texture: "qlv7_08",
            answers: ["自転車", "ローラースケート", "徒歩", "小走り"],
        },
        Question {
            voice: "VT_QL7_1009",
            texture: "qlv7_09",
            answers: ["バイク", "自転車", "工事の看板", "スポーツカー"],
        },
        Question {
            voice: "VT_QL7_1010",
            texture: "qlv7_10",
            answers: [
                "おととい来やがれ",
                "目障りだ、失せろ",
                "凄い漢だ",
                "私一人で充分だ",
            ],
        },
        Question {
            voice: "VT_QL7_1011",
            texture: "qlv7_11",
            answers: [
                "着衣の重要性",
                "水着の重要性",
                "メイド服の重要性",
                "ねこ耳の重要性",
            ],
        },
        Question {
            voice: "VT_QL7_1012",
            texture: "qlv7_12",
            answers: ["アイスコーヒー", "オムライス", "苺ショート", "うどんの出汁"],
        },
        Question {
            voice: "VT_QL7_1013",
            texture: "qlv7_13",
            answers: [
                "アバンチュール",
                "レッセフェール",
                "スリール",
                "ジョリフィーユ",
            ],
        },
        Question {
            voice: "VT_QL7_1014",
            texture: "qlv7_14",
            answers: ["1,800円", "2,980円", "49,800円", "980円"],
        },
        Question {
            voice: "VT_QL7_1015",
            texture: "qlv7_15",
            answers: ["フキフキ！", "ヌギヌギ！", "ブギウギ！", "ブリキー！"],
        },
        Question {
            voice: "VT_QL7_1016",
            texture: "qlv7_16",
            answers: ["部活メンバー", "父親", "暗黒四天王", "校長"],
        },
        Question {
            voice: "VT_QL7_1017",
            texture: "qlv7_17",
            answers: ["沙都子と梨花", "レナと魅音", "魅音と詩音", "富竹と鷹野"],
        },
        Question {
            voice: "VT_QL7_1018",
            texture: "qlv7_18",
            answers: ["5分以上", "1分以内", "3分以上", "気の済むまで"],
        },
        Question {
            voice: "VT_QL7_1019",
            texture: "qlv7_19",
            answers: ["にーにー", "あにぃ", "にいや", "あにしゃん"],
        },
        Question {
            voice: "VT_QL7_1020",
            texture: "qlv7_20",
            answers: ["そぼろ弁当", "肉巻きお握り", "熊肉重", "サンドイッチ"],
        },
        Question {
            voice: "VT_QL7_1021",
            texture: "qlv7_21",
            answers: ["彩り", "炭水化物", "緑黄色野菜", "バランス"],
        },
        Question {
            voice: "VT_QL7_1022",
            texture: "qlv7_22",
            answers: ["梨花", "沙都子", "圭一", "沙都子＆梨花"],
        },
        Question {
            voice: "VT_QL7_1023",
            texture: "qlv7_23",
            answers: ["野菜炒め", "チースー飯", "五目うま煮ご飯", "酢豚"],
        },
        Question {
            voice: "VT_QL7_1024",
            texture: "qlv7_24",
            answers: ["トイレ", "せっちん", "厠", "お便所"],
        },
        Question {
            voice: "VT_QL7_1025",
            texture: "qlv7_25",
            answers: ["二晩", "七日", "半日", "一ヶ月"],
        },
        Question {
            voice: "VT_QL7_1026",
            texture: "qlv7_26",
            answers: ["パーティー", "オフ会", "三者面談", "親族会議"],
        },
        Question {
            voice: "VT_QL7_1027",
            texture: "qlv7_27",
            answers: ["ゴルフクラブ", "金属バット", "鉄パイプ", "水鉄砲"],
        },
        Question {
            voice: "VT_QL7_1028",
            texture: "qlv7_28",
            answers: ["ガソリンスタンド", "スーパー", "市民プール", "野球場"],
        },
        Question {
            voice: "VT_QL7_1029",
            texture: "qlv7_29",
            answers: ["弾丸直球", "ひげそり投法", "フォークの神様", "ケンカ投法"],
        },
        Question {
            voice: "VT_QL7_1030",
            texture: "qlv7_30",
            answers: ["野球", "サッカー", "バスケットボール", "ポケバイレース"],
        },
        Question {
            voice: "VT_QL7_1031",
            texture: "qlv7_31",
            answers: [
                "焼き肉食い放題",
                "デカ盛り食堂",
                "スイーツ食い放題",
                "アイドル握手会",
            ],
        },
        Question {
            voice: "VT_QL7_1032",
            texture: "qlv7_32",
            answers: ["洋菓子", "骨付きカルビ", "ロース", "ちゃんこ鍋"],
        },
        Question {
            voice: "VT_QL7_1033",
            texture: "qlv7_33",
            answers: [
                "ホームラン",
                "満塁ホームラン",
                "犠牲フライ",
                "ツーベースヒット",
            ],
        },
        Question {
            voice: "VT_QL7_1034",
            texture: "qlv7_34",
            answers: [
                "バーベキュー大会",
                "焼き芋大会",
                "お寿司食べ放題",
                "スイーツ食べ放題",
            ],
        },
        Question {
            voice: "VT_QL7_1035",
            texture: "qlv7_35",
            answers: ["入谷", "入山", "入川", "入岡"],
        },
        Question {
            voice: "VT_QL7_1036",
            texture: "qlv7_36",
            answers: ["転校", "入院", "事故死", "留学"],
        },
        Question {
            voice: "VT_QL7_1037",
            texture: "qlv7_37",
            answers: ["マスコット", "生き字引", "祟り", "良心"],
        },
        Question {
            voice: "VT_QL7_1038",
            texture: "qlv7_38",
            answers: ["可憐なメイド", "魔法少女", "女神", "村のアイドル"],
        },
        Question {
            voice: "VT_QL7_1039",
            texture: "qlv7_39",
            answers: [
                "幽霊マネージャー",
                "幻の控え投手",
                "ピンチヒッター",
                "真の監督",
            ],
        },
        Question {
            voice: "VT_QL7_1040",
            texture: "qlv7_40",
            answers: ["２階の雨どい", "物置の屋根", "重機の隙間", "側溝の奥"],
        },
        Question {
            voice: "VT_QL7_1041",
            texture: "qlv7_41",
            answers: ["マネージャー", "彼氏", "お兄さん", "保護者"],
        },
        Question {
            voice: "VT_QL7_1042",
            texture: "qlv7_42",
            answers: ["前原屋敷", "前原ホテル", "前原庭園", "前原豪邸"],
        },
        Question {
            voice: "VT_QL7_1043",
            texture: "qlv7_43",
            answers: ["入江", "詩音", "知恵", "魅音"],
        },
        Question {
            voice: "VT_QL7_1044",
            texture: "qlv7_44",
            answers: ["使い", "生まれ変わり", "影", "しもべ"],
        },
        Question {
            voice: "VT_QL7_1045",
            texture: "qlv7_45",
            answers: ["魅音", "レナ", "沙都子", "圭一"],
        },
        Question {
            voice: "VT_QL7_1046",
            texture: "qlv7_46",
            answers: [
                "父が起きなかった",
                "母が熱を出した",
                "新刊が落ちた",
                "エジプト旅行中",
            ],
        },
        Question {
            voice: "VT_QL7_1047",
            texture: "qlv7_47",
            answers: [
                "シャーロック前原",
                "アルセーヌ前原",
                "前原コゴロウ",
                "江戸川ケーイチ",
            ],
        },
        Question {
            voice: "VT_QL7_1048",
            texture: "qlv7_48",
            answers: ["悟史", "詩音", "オニカクシ", "オヤシロサマ"],
        },
        Question {
            voice: "VT_QL7_1049",
            texture: "qlv7_49",
            answers: [
                "興宮書房の大石",
                "雛見沢分署の大石",
                "捜査一課の目暮",
                "胃腸の弱い永沢",
            ],
        },
        Question {
            voice: "VT_QL7_1050",
            texture: "qlv7_50",
            answers: ["神隠し", "金縛り", "幽体離脱", "記憶喪失"],
        },
        Question {
            voice: "VT_QL7_1051",
            texture: "qlv7_51",
            answers: ["梨花の両親", "沙都子の叔母", "悟史", "現場監督"],
        },
        Question {
            voice: "VT_QL7_1052",
            texture: "qlv7_52",
            answers: ["現場監督", "梨花の父親", "梨花の母親", "分校の先生"],
        },
        Question {
            voice: "VT_QL7_1053",
            texture: "qlv7_53",
            answers: ["新聞紙", "包装紙", "ラップ", "タッパー"],
        },
        Question {
            voice: "VT_QL7_1054",
            texture: "qlv7_54",
            answers: ["バット", "ゴルフクラブ", "鉄パイプ", "ぬいぐるみ"],
        },
        Question {
            voice: "VT_QL7_1055",
            texture: "qlv7_55",
            answers: ["2人", "4人", "5人", "1人"],
        },
        Question {
            voice: "VT_QL7_1056",
            texture: "qlv7_56",
            answers: ["ごめんなさい", "喉が痒い…", "嘘だ！", "俺じゃない…"],
        },
        Question {
            voice: "VT_QL7_1057",
            texture: "qlv7_57",
            answers: ["白", "黒", "ツートン", "グレー"],
        },
        Question {
            voice: "VT_QL7_1058",
            texture: "qlv7_58",
            answers: [
                "僕達はもう知ってる",
                "君は何も知らない",
                "僕らの雛見沢",
                "僕はきっと知ってる",
            ],
        },
        Question {
            voice: "VT_QL7_1059",
            texture: "qlv7_59",
            answers: ["あの日へ", "光へ", "あの日ね", "汝、奉り給え"],
        },
        Question {
            voice: "VT_QL7_1060",
            texture: "qlv7_60",
            answers: ["you", "me", "hope", "Answer"],
        },
    ],
    [
        Question {
            voice: "VT_QL8_1001",
            texture: "qlv8_01",
            answers: ["イカリング", "豚肉", "タマネギ", "飛騨牛"],
        },
        Question {
            voice: "VT_QL8_1002",
            texture: "qlv8_02",
            answers: ["明石", "モーリタニア", "三原", "日間賀島"],
        },
        Question {
            voice: "VT_QL8_1003",
            texture: "qlv8_03",
            answers: [
                "実に美味しそう！",
                "お姉と分けっこ",
                "とっても可愛い！！",
                "活きがいい！",
            ],
        },
        Question {
            voice: "VT_QL8_1004",
            texture: "qlv8_04",
            answers: ["富竹と鷹野", "大石と熊谷", "知恵と入江", "沙都子と悟史"],
        },
        Question {
            voice: "VT_QL8_1005",
            texture: "qlv8_05",
            answers: ["お月様", "蛍火", "花火", "演舞"],
        },
        Question {
            voice: "VT_QL8_1006",
            texture: "qlv8_06",
            answers: ["祭具殿", "賽銭箱", "酒蔵", "社務所"],
        },
        Question {
            voice: "VT_QL8_1007",
            texture: "qlv8_07",
            answers: ["拷問器具", "大人向けの雑誌", "御神輿", "ランディ君人形"],
        },
        Question {
            voice: "VT_QL8_1008",
            texture: "qlv8_08",
            answers: ["鬼の国", "夢の国", "黄泉の国", "黄金郷"],
        },
        Question {
            voice: "VT_QL8_1009",
            texture: "qlv8_09",
            answers: ["猫さん", "圭一", "鷹野", "ワンワン"],
        },
        Question {
            voice: "VT_QL8_1010",
            texture: "qlv8_10",
            answers: ["村長", "詩音の父", "詩音の母", "梨花"],
        },
        Question {
            voice: "VT_QL8_1011",
            texture: "qlv8_11",
            answers: ["レナ", "詩音", "沙都子", "魅音"],
        },
        Question {
            voice: "VT_QL8_1012",
            texture: "qlv8_12",
            answers: [
                "神社の裏手の小屋",
                "沙都子の家の隣",
                "神社の集会所",
                "興宮のホテル",
            ],
        },
        Question {
            voice: "VT_QL8_1013",
            texture: "qlv8_13",
            answers: ["断った", "飲み干した", "おかわりした", "頭から被った"],
        },
        Question {
            voice: "VT_QL8_1014",
            texture: "qlv8_14",
            answers: [
                "監視カメラ",
                "沙都子のトラップ",
                "飛び出し坊や",
                "ペットボトル",
            ],
        },
        Question {
            voice: "VT_QL8_1015",
            texture: "qlv8_15",
            answers: ["松茸", "タラの芽", "タマゴタケ", "トリュフ"],
        },
        Question {
            voice: "VT_QL8_1016",
            texture: "qlv8_16",
            answers: ["百万円", "無料", "米一俵", "五百円"],
        },
        Question {
            voice: "VT_QL8_1017",
            texture: "qlv8_17",
            answers: ["鬼", "オヤシロさま", "悟史", "宇宙人"],
        },
        Question {
            voice: "VT_QL8_1018",
            texture: "qlv8_18",
            answers: ["魅音", "詩音", "眞音", "瑠音"],
        },
        Question {
            voice: "VT_QL8_1019",
            texture: "qlv8_19",
            answers: ["風邪", "頭痛", "骨折", "麻疹"],
        },
        Question {
            voice: "VT_QL8_1020",
            texture: "qlv8_20",
            answers: ["叔父", "叔母", "兄", "両親"],
        },
        Question {
            voice: "VT_QL8_1021",
            texture: "qlv8_21",
            answers: ["弟", "兄", "従兄弟", "叔父"],
        },
        Question {
            voice: "VT_QL8_1022",
            texture: "qlv8_22",
            answers: ["椿", "赤坂", "各務", "大石"],
        },
        Question {
            voice: "VT_QL8_1023",
            texture: "qlv8_23",
            answers: ["監督", "大石", "村長", "知恵"],
        },
        Question {
            voice: "VT_QL8_1024",
            texture: "qlv8_24",
            answers: ["吊り橋", "崖", "屋上", "歩道"],
        },
        Question {
            voice: "VT_QL8_1025",
            texture: "qlv8_25",
            answers: ["梨花", "魅音", "悟史", "沙都子の母"],
        },
        Question {
            voice: "VT_QL8_1026",
            texture: "qlv8_26",
            answers: ["睡眠薬", "交通事故", "多臓器不全", "窒息"],
        },
        Question {
            voice: "VT_QL8_1027",
            texture: "qlv8_27",
            answers: ["バスタオル", "パジャマ", "レインコート", "普段着"],
        },
        Question {
            voice: "VT_QL8_1028",
            texture: "qlv8_28",
            answers: ["6月22日未明", "6月27日未明", "10月4日未明", "7月26日未明"],
        },
        Question {
            voice: "VT_QL8_1029",
            texture: "qlv8_29",
            answers: ["昭和53年", "昭和60年", "昭和47年", "平成元年"],
        },
        Question {
            voice: "VT_QL8_1030",
            texture: "qlv8_30",
            answers: ["犬飼", "羊飼", "牛飼", "馴鹿"],
        },
        Question {
            voice: "VT_QL8_1031",
            texture: "qlv8_31",
            answers: ["雪絵", "美幸", "美雪", "梨花"],
        },
        Question {
            voice: "VT_QL8_1032",
            texture: "qlv8_32",
            answers: [
                "精神を鋭利に",
                "感覚を研ぎ澄ませ",
                "空間を把握しろ",
                "アンテナを張れ",
            ],
        },
        Question {
            voice: "VT_QL8_1033",
            texture: "qlv8_33",
            answers: ["甲乙丙", "大中小", "松竹梅", "イロハ"],
        },
        Question {
            voice: "VT_QL8_1034",
            texture: "qlv8_34",
            answers: ["人食い鬼", "かまいたち", "烏天狗", "悪戯狸"],
        },
        Question {
            voice: "VT_QL8_1035",
            texture: "qlv8_35",
            answers: ["安いホテル", "キャンプ", "民宿", "興宮署"],
        },
        Question {
            voice: "VT_QL8_1036",
            texture: "qlv8_36",
            answers: ["クシャミ", "イカサマ", "急用で帰った", "失神"],
        },
        Question {
            voice: "VT_QL8_1037",
            texture: "qlv8_37",
            answers: ["高田馬場", "八丁堀", "門前仲町", "浦安"],
        },
        Question {
            voice: "VT_QL8_1038",
            texture: "qlv8_38",
            answers: [
                "営林署の器材小屋",
                "分校の資材置き場",
                "谷河内の廃屋",
                "園崎家の地下",
            ],
        },
        Question {
            voice: "VT_QL8_1039",
            texture: "qlv8_39",
            answers: ["3.5本", "2.5本", "1本", "7本"],
        },
        Question {
            voice: "VT_QL8_1040",
            texture: "qlv8_40",
            answers: ["銀シャリ", "焼肉", "ハンバーガー", "麦飯"],
        },
        Question {
            voice: "VT_QL8_1041",
            texture: "qlv8_41",
            answers: [
                "煎餅の値引き率",
                "餃子のうまさ",
                "赤い物の速度",
                "外来の自己負担額",
            ],
        },
        Question {
            voice: "VT_QL8_1042",
            texture: "qlv8_42",
            answers: ["魅音", "沁子", "茜", "葛西"],
        },
        Question {
            voice: "VT_QL8_1043",
            texture: "qlv8_43",
            answers: ["悟史", "詩音", "沙都子", "鉄平"],
        },
        Question {
            voice: "VT_QL8_1044",
            texture: "qlv8_44",
            answers: ["アスパラガス", "レンコン", "トマト", "ブロッコリー"],
        },
        Question {
            voice: "VT_QL8_1045",
            texture: "qlv8_45",
            answers: ["スタンガン", "バット", "包丁", "高枝切りばさみ"],
        },
        Question {
            voice: "VT_QL8_1046",
            texture: "qlv8_46",
            answers: [
                "フワラズの勾玉",
                "魔法の鏡",
                "オヤシロステッキ",
                "ニパニパワー",
            ],
        },
        Question {
            voice: "VT_QL8_1047",
            texture: "qlv8_47",
            answers: ["クッキー", "お弁当", "カップケーキ", "おはぎ"],
        },
        Question {
            voice: "VT_QL8_1048",
            texture: "qlv8_48",
            answers: ["富竹", "レナ", "鷹野", "圭一"],
        },
        Question {
            voice: "VT_QL8_1049",
            texture: "qlv8_49",
            answers: ["レナ", "鷹野", "富竹", "入江"],
        },
        Question {
            voice: "VT_QL8_1050",
            texture: "qlv8_50",
            answers: ["方位磁石", "腕時計", "ペンデュラム", "カーナビ"],
        },
        Question {
            voice: "VT_QL8_1051",
            texture: "qlv8_51",
            answers: [
                "現代メイド理論",
                "近代メイド図鑑",
                "メイドマガジン",
                "メイド＆タイツ",
            ],
        },
        Question {
            voice: "VT_QL8_1052",
            texture: "qlv8_52",
            answers: ["意外に家庭的", "ポニーテール", "義理の妹", "実は男の娘"],
        },
        Question {
            voice: "VT_QL8_1053",
            texture: "qlv8_53",
            answers: ["沙都子", "悟史", "魅音", "監督"],
        },
        Question {
            voice: "VT_QL8_1054",
            texture: "qlv8_54",
            answers: ["水鉄砲対決", "わんこそば対決", "流しそうめん対決", "缶蹴り"],
        },
        Question {
            voice: "VT_QL8_1055",
            texture: "qlv8_55",
            answers: ["引き分け", "圭一の勝利", "レナの勝利", "梨花の勝利"],
        },
        Question {
            voice: "VT_QL8_1056",
            texture: "qlv8_56",
            answers: ["勝利宣言", "敗北宣言", "リベンジ宣言", "プロポーズ"],
        },
        Question {
            voice: "VT_QL8_1057",
            texture: "qlv8_57",
            answers: ["ガソリン", "除草剤", "石灰岩", "灯油"],
        },
        Question {
            voice: "VT_QL8_1058",
            texture: "qlv8_58",
            answers: ["スクール水着", "メイド服", "巫女服", "魔法少女"],
        },
        Question {
            voice: "VT_QL8_1059",
            texture: "qlv8_59",
            answers: ["富田と岡村", "荒川と利根川", "花田と藤田", "大石と富竹"],
        },
        Question {
            voice: "VT_QL8_1060",
            texture: "qlv8_60",
            answers: [
                "日替わりデザート",
                "日替わりランチ",
                "オムライス",
                "ナポリタン",
            ],
        },
    ],
];
pub(super) const SPECIAL: [Question; 10] = [
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["1問目", "2問目", "3問目", "0問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["2問目", "1問目", "3問目", "4問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["3問目", "2問目", "5問目", "4問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["4問目", "2問目", "3問目", "5問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["5問目", "4問目", "6問目", "7問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["6問目", "5問目", "7問目", "8問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["7問目", "5問目", "6問目", "8問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["8問目", "6問目", "7問目", "9問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["9問目", "7問目", "8問目", "10問目"],
    },
    Question {
        voice: "VT_QL0_1001",
        texture: "qlv0_01",
        answers: ["10問目", "11問目", "8問目", "9問目"],
    },
];

/// Relocated native1b7420: nine character contexts, five correct/five incorrect.
pub(super) const REACTIONS: [[&str; 10]; 9] = [
    [
        "VT_rika_1001",
        "VT_rika_1002",
        "VT_rika_1003",
        "VT_rika_1004",
        "VT_rika_1005",
        "VT_rika_2001",
        "VT_rika_2002",
        "VT_rika_2003",
        "VT_rika_2004",
        "VT_rika_2005",
    ],
    [
        "VT_miyoko_1001",
        "VT_miyoko_1002",
        "VT_miyoko_1003",
        "VT_miyoko_1004",
        "VT_miyoko_1005",
        "VT_miyoko_2001",
        "VT_miyoko_2002",
        "VT_miyoko_2003",
        "VT_miyoko_2004",
        "VT_miyoko_2005",
    ],
    [
        "VT_hanyu_1001",
        "VT_hanyu_1002",
        "VT_hanyu_1003",
        "VT_hanyu_1004",
        "VT_hanyu_1005",
        "VT_hanyu_2001",
        "VT_hanyu_2002",
        "VT_hanyu_2003",
        "VT_hanyu_2004",
        "VT_hanyu_2005",
    ],
    [
        "VT_Brika_1001",
        "VT_Brika_1002",
        "VT_Brika_1003",
        "VT_Brika_1004",
        "VT_Brika_1005",
        "VT_Brika_2001",
        "VT_Brika_2002",
        "VT_Brika_2003",
        "VT_Brika_2004",
        "VT_Brika_2005",
    ],
    [
        "VT_Bmiyoko_1001",
        "VT_Bmiyoko_1002",
        "VT_Bmiyoko_1003",
        "VT_Bmiyoko_1004",
        "VT_Bmiyoko_1005",
        "VT_Bmiyoko_2001",
        "VT_Bmiyoko_2002",
        "VT_Bmiyoko_2003",
        "VT_Bmiyoko_2004",
        "VT_Bmiyoko_2005",
    ],
    [
        "VT_Bhanyu_1001",
        "VT_Bhanyu_1002",
        "VT_Bhanyu_1003",
        "VT_Bhanyu_1004",
        "VT_Bhanyu_1005",
        "VT_Bhanyu_2001",
        "VT_Bhanyu_2002",
        "VT_Bhanyu_2003",
        "VT_Bhanyu_2004",
        "VT_Bhanyu_2005",
    ],
    [
        "VT_hanyu_1001",
        "VT_hanyu_1002",
        "VT_hanyu_1003",
        "VT_hanyu_1004",
        "VT_hanyu_1005",
        "VT_hanyu_2001",
        "VT_hanyu_2002",
        "VT_hanyu_2003",
        "VT_hanyu_2004",
        "VT_hanyu_2005",
    ],
    [
        "QKEI_0000",
        "QKEI_0010",
        "QKEI_0020",
        "QKEI_0030",
        "QKEI_0040",
        "QKEI_0050",
        "QKEI_0060",
        "QKEI_0070",
        "QKEI_0080",
        "QKEI_0090",
    ],
    [
        "QREN_0000",
        "QREN_0010",
        "QREN_0020",
        "QREN_0030",
        "QREN_0040",
        "QREN_0050",
        "QREN_0060",
        "QREN_0070",
        "QREN_0080",
        "QREN_0090",
    ],
];
