//! Match only the verified Dead Space linear fog source fragment.
//! Its zero range means fog is disabled, as on the accepted GLES1 path.
//! The guard prevents disabled infinite-reciprocal fog from zeroing fragments.
//! Independent native projection repair handles the prologue far-plane clipping.
//! Valid positive fog ranges retain original arithmetic.
pub(crate) const ORIGINAL: &str = r#"uniform highp vec2 g_Fog;
uniform highp vec3 g_FogColour;
vec4 Fog(in float z)
{
	highp float fog = clamp((g_Fog.y - z) * g_Fog.x, 0.0, 1.0);
   return vec4((1.0 - fog) * g_FogColour, fog);
}
"#;
pub(crate) fn patch(source: &str) -> Option<String> {
    if !source.contains(ORIGINAL) {
        return None;
    }
    let corrected=ORIGINAL.replace("vec4 Fog(in float z)","uniform bool touchHLE_DsFogDisabled;\nvec4 Fog(in float z)")
        .replacen("{\n", "{\n    if (touchHLE_DsFogDisabled || g_Fog.y <= 0.0) return vec4(0.0, 0.0, 0.0, 1.0);\n",1);
    Some(source.replace(ORIGINAL, &corrected))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_verified_fragment_is_replaced_and_patch_is_idempotent() {
        assert!(patch("void main(){}").is_none());
        assert!(patch(&ORIGINAL.replace("highp vec2", "mediump vec2")).is_none());
        let source = format!("// prefix\n{}// suffix", ORIGINAL);
        let fixed = patch(&source).unwrap();
        assert!(fixed.starts_with("// prefix\n") && fixed.ends_with("// suffix"));
        assert!(fixed.contains("touchHLE_DsFogDisabled || g_Fog.y <= 0.0"));
        assert!(fixed.contains("clamp((g_Fog.y - z) * g_Fog.x, 0.0, 1.0)"));
        assert!(patch(&fixed).is_none());
    }
}
