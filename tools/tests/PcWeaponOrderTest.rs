#[path = "../../src/ds_hud_commands.rs"]
mod ds_hud_commands;
#[path = "../../src/ds_weapon_input.rs"]
mod ds_weapon_input;
#[path = "../../src/fs/ds_weapon_layout.rs"]
mod layout;
mod window {
    pub fn ds_prompt_key(action: &str) -> String { format!("REBOUND_{action}") }
    pub fn ds_prompt_button(_: &str) -> String { "A".into() }
    pub fn ds_pad_mode_toggle() -> bool {false}
    pub fn ds_keyboard_alt_hold() -> bool {true}
    pub fn ds_locator_prompt_keys() -> Vec<String> {vec!["Ctrl".into(),"G".into(),"B".into()]}
}
#[path = "../../src/ds_tutorial_text.rs"]
mod tutorial;

#[test]
fn dpad_and_keyboard_share_native_order() {
    use ds_hud_commands::HudCommand;
    for direction in 0..4 {
        let mut pad = ds_weapon_input::PadWeapons::new();
        let command = pad.update(direction, true, 0).unwrap();
        assert_eq!(command.weapon_index(), Some(direction as u32));
        assert_eq!(command, HudCommand::from_key(&(direction + 1).to_string()).unwrap());
        assert_eq!(pad.tick(449), None);
        assert_eq!(pad.tick(450).unwrap().weapon_index(), Some(4));
    }
}
#[test]
fn tutorial_uses_rebound_keys_and_requested_names() {
    let (text, keys) = tutorial::content("TUTORIAL_WEAPON_SELECT_TEXT", "old");
    let mut offset = 0;
    for (index, name) in ["等离子切割器", "切割锯", "线性枪", "岩石取芯器", "大脉冲"].into_iter().enumerate() {
        let tail = &text[offset..]; offset += tail.find(name).unwrap() + name.len();
        assert!(text.contains(&format!("REBOUND_weapon{}", index + 1)));
        assert_eq!(keys[index], format!("REBOUND_weapon{}", index + 1));
    }
    assert!(text.contains("450ms"));
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {u32::from_le_bytes(bytes[at..at+4].try_into().unwrap())}
#[test]
fn original_native_resource_gets_only_verified_geometry_edits() {
    let original = std::fs::read(std::env::var("PC_WEAPON_LAYOUT_FIXTURE").expect("Set PC_WEAPON_LAYOUT_FIXTURE to the read-only original layout resource")).unwrap();
    let patched = layout::patch(&original).unwrap();
    assert_eq!(patched.len(), original.len());
    let mut cursor = 8; let(mut data, mut table) = (0,0); let mut data_end = 0;
    while cursor < original.len() {
        let end = cursor + 12 + u32_at(&original,cursor+4) as usize;
        if &original[cursor..cursor+4] == b"DATA" {data=cursor+12;data_end=end;}
        else {assert_eq!(&original[cursor..end], &patched[cursor..end], "unrelated SBIN section changed");}
        if &original[cursor..cursor+4] == b"OHDR" {table=cursor+12;}
        cursor=(end+3)&!3;
    }
    for refs in [[1448,1449,1447,1450,1451],[2319,2320,2328,2321,2334]] {
        for (slot, reference) in refs.into_iter().enumerate() {
            let at=data+(u32_at(&patched,table+reference*4)>>3) as usize+8;
            let y=f32::from_bits(u32_at(&patched,at+4));
            assert_eq!(y,10.0+slot as f32*60.0);
            assert_eq!(f32::from_bits(u32_at(&patched,at)), -80.0);
            assert_eq!(f32::from_bits(u32_at(&patched,at+20)),y+60.0);
            assert_eq!(&original[at+8..at+16],&patched[at+8..at+16],"anchors changed");
        }
    }
    for reference in [1458,2327] {
        let at=data+(u32_at(&patched,table+reference*4)>>3) as usize+8;
        assert_eq!(f32::from_bits(u32_at(&patched,at)),-178.0);
        assert_eq!(f32::from_bits(u32_at(&patched,at+16)),-98.0);
        assert_eq!(&original[at+4..at+16],&patched[at+4..at+16]);
        assert_eq!(&original[at+20..at+32],&patched[at+20..at+32]);
    }
    assert_eq!(original.iter().zip(&patched).filter(|(a,b)|a!=b).count(), (0..original.len()).filter(|i|original[*i]!=patched[*i] && (*i>=data&&*i<data_end||*i>=data-4&&*i<data)).count());
    // Checksum-valid, but wrong entity references are still rejected.
    let mut corrupt=original.clone();corrupt[data+86932+36]^=1;
    assert!(layout::patch(&corrupt).is_none());
    assert!(layout::patch(&original[..original.len()-1]).is_none());
    assert!(layout::patch(&[]).is_none());
}
