//! A context-owned shader for the existing GPU card bitmaps and geometry.
use crate::gles::{gles2_raw as gl,present_attrib::SavedAttrib,GLES};

#[derive(Default)]
pub(super) struct State {program:u32,vbo:u32,opacity:i32,failed:bool}
pub(super) struct Saved {program:i32,array:i32,attributes:[SavedAttrib;2],equation:[i32;2]}
impl State {
    pub(super) unsafe fn initialize(&mut self,g:&mut dyn GLES)->bool {
        if self.failed {self.failed=true;return false;}
        if self.program!=0 {return true;}
        let mut shaders=[0u32;2];
        for (index,(kind,source)) in [
            (gl::VERTEX_SHADER,"attribute vec2 aPos; attribute vec2 aUV; varying vec2 vUV; void main(){gl_Position=vec4(aPos,0.0,1.0);vUV=aUV;}"),
            (gl::FRAGMENT_SHADER,"precision mediump float;\nvarying vec2 vUV; uniform sampler2D uTex; uniform float uOpacity; void main(){vec4 t=texture2D(uTex,vUV);gl_FragColor=vec4(t.rgb,t.a*uOpacity);}"),
        ].into_iter().enumerate() {
            let shader=g.CreateShader(kind);shaders[index]=shader;
            let pointer=source.as_ptr().cast();let length=source.len() as i32;
            g.ShaderSource(shader,1,&pointer,&length);g.CompileShader(shader);
            let mut success=0;g.GetShaderiv(shader,gl::COMPILE_STATUS,&mut success);
            if success==0 {
                for shader in shaders {if shader!=0 {g.DeleteShader(shader);}}
                self.failed=true;return false;
            }
        }
        let program=g.CreateProgram();for shader in shaders {g.AttachShader(program,shader);}
        g.BindAttribLocation(program,6,b"aPos\0".as_ptr().cast());
        g.BindAttribLocation(program,7,b"aUV\0".as_ptr().cast());
        g.LinkProgram(program);for shader in shaders {g.DeleteShader(shader);}
        let mut success=0;g.GetProgramiv(program,gl::LINK_STATUS,&mut success);
        if success==0 {g.DeleteProgram(program);self.failed=true;return false;}
        let opacity=g.GetUniformLocation(program,b"uOpacity\0".as_ptr().cast());
        let texture=g.GetUniformLocation(program,b"uTex\0".as_ptr().cast());
        if opacity<0 || texture<0 {g.DeleteProgram(program);self.failed=true;return false;}
        let mut old=0;g.GetIntegerv(gl::CURRENT_PROGRAM,&mut old);g.UseProgram(program);
        g.Uniform1i(texture,0);g.UseProgram(old as u32);
        g.GenBuffers(1,&mut self.vbo);
        if self.vbo==0 {g.DeleteProgram(program);self.failed=true;return false;}
        self.program=program;self.opacity=opacity;
        log!("[DSoverlay-gpu] ES2 context-owned card shader ready program={}",program);
        true
    }
    pub(super) unsafe fn begin(&mut self,g:&mut dyn GLES)->Option<Saved> {
        if !self.initialize(g) {return None;}
        let mut old_program=0;let mut old_array=0;
        g.GetIntegerv(gl::CURRENT_PROGRAM,&mut old_program);g.GetIntegerv(gl::ARRAY_BUFFER_BINDING,&mut old_array);
        let attributes=[SavedAttrib::capture(g,6),SavedAttrib::capture(g,7)];
        let mut equation=[0;2];g.GetIntegerv(gl::BLEND_EQUATION_RGB,&mut equation[0]);g.GetIntegerv(gl::BLEND_EQUATION_ALPHA,&mut equation[1]);
        g.BlendEquation(gl::FUNC_ADD);
        g.UseProgram(self.program);g.BindBuffer(gl::ARRAY_BUFFER,self.vbo);
        g.BufferData(gl::ARRAY_BUFFER,96,std::ptr::null(),gl::STREAM_DRAW);
        g.EnableVertexAttribArray(6);g.EnableVertexAttribArray(7);
        g.VertexAttribPointer(6,2,gl::FLOAT,gl::FALSE,16,std::ptr::null());
        g.VertexAttribPointer(7,2,gl::FLOAT,gl::FALSE,16,8usize as *const _);
        Some(Saved {program:old_program,array:old_array,attributes,equation})
    }
    pub(super) unsafe fn draw(&self,g:&mut dyn GLES,vertices:&[f32;12],opacity:f32) {
        let uv=[0.,0.,0.,1.,1.,0.,1.,0.,0.,1.,1.,1.];let mut data=[0f32;24];
        for i in 0..6 {data[i*4]=vertices[i*2];data[i*4+1]=vertices[i*2+1];data[i*4+2]=uv[i*2];data[i*4+3]=uv[i*2+1];}
        g.BufferSubData(gl::ARRAY_BUFFER,0,96,data.as_ptr().cast());
        g.Uniform1f(self.opacity,opacity);g.DrawArrays(gl::TRIANGLES,0,6);
    }
    pub(super) unsafe fn end(&self,g:&mut dyn GLES,saved:Saved) {
        for attribute in saved.attributes {attribute.restore(g);}
        g.BlendEquationSeparate(saved.equation[0] as u32,saved.equation[1] as u32);
        g.BindBuffer(gl::ARRAY_BUFFER,saved.array as u32);g.UseProgram(saved.program as u32);
    }
    pub(super) unsafe fn release(&mut self,g:&mut dyn GLES) {
        if self.program!=0 {g.DeleteProgram(self.program);}if self.vbo!=0 {g.DeleteBuffers(1,&self.vbo);}
        *self=Self::default();
    }
}
