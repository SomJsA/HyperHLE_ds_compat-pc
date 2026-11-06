//! Final-output pointer geometry: its tip is the exact SDL click position.
pub(crate) fn mouse_at(window:(u32,u32),drawable:(u32,u32),viewport:(u32,u32,u32,u32),mouse:(i32,i32),pressed:bool)->Option<(f32,f32,bool)> {
    let x=mouse.0 as f32*drawable.0 as f32/window.0.max(1) as f32;
    let y=mouse.1 as f32*drawable.1 as f32/window.1.max(1) as f32;
    let (vx,vy,vw,vh)=viewport;
    (x>=vx as f32 && y>=vy as f32 && x<(vx+vw) as f32 && y<(vy+vh) as f32).then_some((x,y,pressed))
}
/// Opaque underlay for the software cursor. The guest can still draw its own
/// menu pointer underneath; this disc hides that pointer while the cyan
/// software pointer is active, instead of showing two cursors at once.
pub(crate) fn backing(viewport: (u32,u32,u32,u32), at: (f32,f32,bool)) -> ([f32;24],[f32;4]) {
    let (vx,vy,w,h)=viewport;
    let scale=(h as f32/1080.0).clamp(1.0,2.5);
    let radius=20.0*scale;
    let mut v=[0.;24];
    // Four triangles fit the fixed 12-vertex backing buffer.
    const N:usize=4;
    let point=|i:usize|{
        let a=i as f32*std::f32::consts::TAU/N as f32;
        ((at.0-vx as f32)+a.cos()*radius,(at.1-vy as f32)+a.sin()*radius)
    };
    for i in 0..N{
        let a=((at.0-vx as f32),(at.1-vy as f32));
        let b=point(i);let c=point((i+1)%N);
        let idx=i*6;
        v[idx]=a.0*2./w.max(1) as f32-1.;v[idx+1]=1.-a.1*2./h.max(1) as f32;
        v[idx+2]=b.0*2./w.max(1) as f32-1.;v[idx+3]=1.-b.1*2./h.max(1) as f32;
        v[idx+4]=c.0*2./w.max(1) as f32-1.;v[idx+5]=1.-c.1*2./h.max(1) as f32;
    }
    (v,[0.008,0.025,0.04,0.96])
}
pub(crate) fn layers(viewport: (u32,u32,u32,u32), at: (f32,f32,bool)) -> [([f32;24],[f32;4]);3] {
    let (vx,vy,w,h)=viewport;
    let scale=(h as f32/1080.0).clamp(1.0,2.5);
    let outer=[[0.,0.],[0.,28.],[6.,22.],[12.,34.],[18.,31.],[12.,20.],[24.,20.]];
    let inner=[[2.,4.],[2.,23.],[7.,18.],[14.,30.],[15.,29.],[9.,18.],[19.,18.]];
    let indices=[0,1,2,0,2,6,2,5,4,2,4,3];
    let vertices=|points: [[f32;2];7]| {
        let mut v=[0.;24];
        for (i,&n) in indices.iter().enumerate() {
            v[i*2]=((at.0-vx as f32)+points[n][0]*scale)*2./w.max(1) as f32-1.;
            v[i*2+1]=1.-((at.1-vy as f32)+points[n][1]*scale)*2./h.max(1) as f32;
        }
        v
    };
    let mut highlight=inner;
    highlight[0]=[3.,6.];highlight[1]=[3.,17.];highlight[2]=[6.,15.];
    let mut tip=vertices(highlight);
    // Remaining triangles degenerate; keep one compact fixed-size draw API.
    for i in 6..24 {tip[i]=tip[i%2];}
    [(vertices(outer),[0.015,0.035,0.05,1.]),
     (vertices(inner),if at.2 {[1.,0.65,0.18,1.]}else{[0.25,0.87,1.,1.]}),
     (tip,[0.9,0.99,1.,1.])]
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn mouse_matches_hidpi_hit_position_and_leaves_bars_to_os() {
        assert_eq!(mouse_at((1280,720),(2560,1440),(0,0,2560,1440),(200,100),true),Some((400.,200.,true)));
        assert_eq!(mouse_at((1280,720),(1920,1080),(150,0,1620,1080),(200,100),false),Some((300.,150.,false)));
        assert_eq!(mouse_at((1280,720),(1920,1080),(150,0,1620,1080),(10,100),false),None);
        assert_eq!(mouse_at((1280,720),(1920,1080),(150,0,1620,1080),(1280,100),false),None);
    }
    #[test] fn hotspot_uses_drawable_coordinates_and_letterbox_offset() {
        for viewport in [(0,0,1920,1080),(120,60,3600,2040),(0,0,960,640)] {
            let at=(viewport.0 as f32+100.,viewport.1 as f32+80.,false);
            let (v,_)=layers(viewport,at)[0];
            assert!(((v[0]+1.)*viewport.2 as f32/2.-100.).abs()<0.001);
            assert!(((1.-v[1])*viewport.3 as f32/2.-80.).abs()<0.001);
            assert!(v.iter().all(|x|x.is_finite()));
        }
        assert_ne!(layers((0,0,1920,1080),(100.,80.,false))[1].1,layers((0,0,1920,1080),(100.,80.,true))[1].1);
    }
}
