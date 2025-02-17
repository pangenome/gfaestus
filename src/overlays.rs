use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Defines the type of mapping from node ID to colors used by an
/// overlay script
pub enum OverlayKind {
    /// Overlay scripts that produce an RGB color for each node
    RGB,
    /// Overlay scripts that produce a single value for each node,
    /// that can then be mapped to a color, e.g. using a perceptual
    /// color scheme
    Value,
}

pub enum OverlayData {
    RGB(Vec<rgb::RGBA<f32>>),
    Value(Vec<f32>),
}

/*
pub type OverlayScriptType<T> = Function<
    RootedThread,
    fn(GraphHandle) -> IO<Function<RootedThread, fn(u64) -> T>>,
>;

impl OverlayKind {
    fn type_for(&self, vm: &Thread) -> ArcType {
        match self {
            OverlayKind::RGB => {
                <OverlayScriptType<(f32, f32, f32)> as VmType>::make_type(vm)
            }
            OverlayKind::Value => {
                <OverlayScriptType<f32> as VmType>::make_type(vm)
            }
        }
    }

    pub fn typecheck_script(vm: &Thread, script: &str) -> Result<OverlayKind> {
        let rgb_type = OverlayKind::RGB.type_for(vm);

        if let Ok(_) = vm.typecheck_str("", script, Some(&rgb_type)) {
            return Ok(OverlayKind::RGB);
        }

        let value_type = OverlayKind::Value.type_for(vm);

        if let Ok(_) = vm.typecheck_str("", script, Some(&value_type)) {
            return Ok(OverlayKind::Value);
        }
        anyhow::bail!("Overlay script has incorrect type")
    }

    pub async fn typecheck_script_(
        vm: &Thread,
        script: &str,
    ) -> Result<OverlayKind> {
        dbg!();
        let rgb_type = OverlayKind::RGB.type_for(vm);

        dbg!();
        if let Ok(_) = vm.typecheck_str_async("", script, Some(&rgb_type)).await
        {
            return Ok(OverlayKind::RGB);
        }

        dbg!();
        let value_type = OverlayKind::Value.type_for(vm);

        if let Ok(_) =
            vm.typecheck_str_async("", script, Some(&value_type)).await
        {
            return Ok(OverlayKind::Value);
        }
        dbg!();

        anyhow::bail!("Overlay script has incorrect type")
    }
}
*/

pub fn hash_node_color(hash: u64) -> (f32, f32, f32) {
    let r_u16 = ((hash >> 32) & 0xFFFFFFFF) as u16;
    let g_u16 = ((hash >> 16) & 0xFFFFFFFF) as u16;
    let b_u16 = (hash & 0xFFFFFFFF) as u16;

    let max = r_u16.max(g_u16).max(b_u16) as f32;
    let r = (r_u16 as f32) / max;
    let g = (g_u16 as f32) / max;
    let b = (b_u16 as f32) / max;
    (r, g, b)
}
