//! PC instructions use semantic actions. The window exports the same parsed
//! assignments it uses for input, so labels also follow user remapping.
fn controller_fire_instruction(toggle:bool,primary:&str)->String {
    if toggle {format!("LT 瞄准，{primary} 切换主副模式，RT 开火。")}
    else {format!("LT 瞄准，{primary} 主攻击，RT 副攻击。")}
}
fn alternate_instruction(toggle:bool,hold:bool,primary:&str)->String {
    let keyboard=if hold {"按住 Space 使用副模式，松开恢复主模式"}else{"按 Space 切换主副模式"};
    format!("键鼠：瞄准时{keyboard}。\n手柄：{}",controller_fire_instruction(toggle,primary))
}
pub(crate) fn content(id:&str,original:&str)->(String,Vec<String>) {
    let k=|action:&str|crate::window::ds_prompt_key(action);
    let p=|action:&str|match crate::window::ds_prompt_button(action).as_str(){"LS"=>"L3".into(),"RS"=>"R3".into(),other=>other.to_owned()};
    // R14 settings are independent. A tutorial must never tell players to
    // shoot with the button which currently selects mode without firing.
    let toggle=crate::window::ds_pad_mode_toggle();
    let fire=if toggle {"RT".to_owned()}else{p("primary")};
    let (text,keys):(String,Vec<String>)=match id {
        "TUTORIAL_MOVE_TEXT" => (format!("键鼠：{} {} {} {} 移动，向前自动跑动，瞄准时步行。\n手柄：左摇杆移动，向前推至边缘奔跑。",k("forward"),k("left"),k("back"),k("right")),vec![k("forward"),k("left"),k("back"),k("right"),"左摇杆".into()]),
        "TUTORIAL_LOOK_TEXT" | "TUTORIAL_REMOTE_SAW_TEXT" | "TUTORIAL_CORER_BEAM_TEXT" => ("移动鼠标／手柄右摇杆控制视角或武器方向。".into(),vec!["鼠标".into(),"右摇杆".into()]),
        "TUTORIAL_AIM_TEXT" => (format!("键鼠：鼠标右键举枪并瞄准电源箱，左键开火。\n手柄：{}",controller_fire_instruction(toggle,&p("primary"))),vec!["鼠标右键".into(),"鼠标左键".into(),"LT".into(),p("primary"),"RT".into()]),
        "TUTORIAL_FIRE_TEXT" | "TUTORIAL_CORER_TEXT" | "TUTORIAL_SAW_SHOT_TEXT" => (format!("瞄准后按鼠标左键／手柄 {} 开火。瞄准敌人的肢体，杀起来更快。",fire),vec!["鼠标左键".into(),fire]),
        "TUTORIAL_ALT_FIRE_PC_TEXT" | "TUTORIAL_ALT_FIRE_TEXT" | "TUTORIAL_TILT_BUTTON" => (alternate_instruction(toggle,crate::window::ds_keyboard_alt_hold(),&p("primary")),vec!["Space".into(),"LT".into(),p("primary"),"RT".into()]),
        "TUTORIAL_KINESIS_TEXT" => (format!("对准物体，按 {}／手柄 {} 使用动态牵引。瞄准后按鼠标左键／{} 发射物体。",k("kinesis"),p("kinesis"),fire),vec![k("kinesis"),p("kinesis"),"鼠标左键".into(),fire]),
        "TUTORIAL_STASIS_TEXT" => (format!("瞄准门，按 {}／手柄 {} 发射静滞能量。",k("stasis"),p("stasis")),vec![k("stasis"),p("stasis")]),
        "TUTORIAL_RELOAD_TEXT" => (format!("按 {}／手柄 {} 装弹。",k("reload"),p("reload")),vec![k("reload"),p("reload")]),
        "TUTORIAL_MELEE_TEXT" => (format!("靠近电源箱，按 {}／手柄 {} 打开箱子，再按 {}／{} 挥动等离子锯。",k("interact"),p("interact"),k("melee"),p("melee")),vec![k("interact"),p("interact"),k("melee"),p("melee")]),
        "TUTORIAL_PLASMA_SAW_TEXT" | "TUTORIAL_STOMP_TEXT" => (format!("靠近目标，按 {}／手柄 {}，根据当前提示挥动等离子锯或踩踏。",k("interact"),p("interact")),vec![k("interact"),p("interact")]),
        "TUTORIAL_QUICK_TURN_TEXT" => (format!("按 {}／手柄 {} 快速转身 180 度。",k("quick_turn"),p("quick_turn")),vec![k("quick_turn"),p("quick_turn")]),
        "TUTORIAL_LOCATOR_TEXT" => {
            let mut keys=crate::window::ds_locator_prompt_keys();
            let text=format!("按 {}／手柄 {} 显示定位路线及当前任务。",keys.join(" / "),p("locator"));
            keys.push(p("locator")); (text,keys)
        },
        "TUTORIAL_BENCH_TEXT" | "TUTORIAL_STORE_TEXT" => (format!("靠近设施，按 {}／手柄 {} 打开界面。\n{}",k("interact"),p("interact"),if id.contains("BENCH"){"使用能量节点升级 RIG 和武器。"}else{"购买护服、弹药和能量节点。"}),vec![k("interact"),p("interact")]),
        "TUTORIAL_WEAPON_SELECT_TEXT" => (format!("{} 等离子切割器 / {} 切割锯 / {} 线性枪 / {} 岩石取芯器 / {} 大脉冲。\n手柄 ↑等离子 / →切割锯 / ↓线性枪 / ←岩石；长按任一方向约450ms选大脉冲。",k("weapon1"),k("weapon2"),k("weapon3"),k("weapon4"),k("weapon5")),vec![k("weapon1"),k("weapon2"),k("weapon3"),k("weapon4"),k("weapon5"),"十字键".into()]),
        "TUTORIAL_ZERO_G_JUMP_TEXT" => ("瞄准能落脚的表面，按 Shift + Space、V 或 Ctrl／手柄 LT + L3 进行零重力跳跃。".into(),vec!["Shift".into(),"Space".into(),"V".into(),"Ctrl".into(),"LT".into(),"L3".into()]),
        _ => (original.to_owned(),Vec::new()),
    };(text,keys)
}

/// The device category belongs to the action, never to the label: keyboard A
/// and controller A must remain visually different after remapping.
pub(crate) fn icon_kind(id:&str,index:usize)->&'static str {
    if id=="TUTORIAL_LOCATOR_TEXT" { return if index<crate::window::ds_locator_prompt_keys().len(){"key"}else{"pad"}; }
    let kinds:&[&str]=match id {
        "TUTORIAL_MOVE_TEXT"=>&["key","key","key","key","stick"],
        "TUTORIAL_LOOK_TEXT"|"TUTORIAL_REMOTE_SAW_TEXT"|"TUTORIAL_CORER_BEAM_TEXT"=>&["mouse","stick"],
        "TUTORIAL_AIM_TEXT"=>&["mouse","mouse","pad","pad","pad"],
        "TUTORIAL_FIRE_TEXT"|"TUTORIAL_CORER_TEXT"|"TUTORIAL_SAW_SHOT_TEXT"=>&["mouse","pad"],
        "TUTORIAL_ALT_FIRE_PC_TEXT"|"TUTORIAL_ALT_FIRE_TEXT"|"TUTORIAL_TILT_BUTTON"=>&["key","pad","pad","pad"],
        "TUTORIAL_KINESIS_TEXT"=>&["key","pad","mouse","pad"],
        "TUTORIAL_MELEE_TEXT"=>&["key","pad","key","pad"],
        "TUTORIAL_WEAPON_SELECT_TEXT"=>&["key","key","key","key","key","dpad"],
        "TUTORIAL_ZERO_G_JUMP_TEXT"=>&["key","key","key","key","pad","pad"],
        _=>&["key","pad"],
    };kinds.get(index).copied().unwrap_or("key")
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn fire_tutorial_preserves_all_four_r14_mode_combinations(){
        for toggle in [false,true] {for hold in [false,true] {
            let text=alternate_instruction(toggle,hold,"Y");
            assert!(text.contains(if hold {"按住 Space"}else{"按 Space 切换"}));
            assert!(text.contains(if toggle {"Y 切换主副模式，RT 开火"}else{"Y 主攻击，RT 副攻击"}));
            assert!(!text.contains(if toggle {"Y 主攻击"}else{"Y 切换主副模式"}));
        }}
    }
    #[test] fn keyboard_and_controller_a_are_distinct(){
        assert_eq!(icon_kind("TUTORIAL_MOVE_TEXT",1),"key");
        assert_eq!(icon_kind("TUTORIAL_MELEE_TEXT",1),"pad");
    }
    #[test] fn pc_control_tutorials_do_not_instruct_touch_or_tilt(){
        for id in ["MOVE","LOOK","AIM","FIRE","MELEE","STASIS","KINESIS","LOCATOR","RELOAD","ALT_FIRE","WEAPON_SELECT","ZERO_G_JUMP"]{
            let(t,icons)=content(&format!("TUTORIAL_{id}_TEXT"),"触摸屏幕");
            for forbidden in ["点击屏幕","滑动屏幕","摇晃设备","倾斜设备","触摸屏幕"]{assert!(!t.contains(forbidden),"{id}: {t}");}
            assert!(!icons.is_empty());
        }
    }
}
