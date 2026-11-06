//! Process-local SDL virtual controller for explicit hidden inbox regression runs.
//! No OS input/driver injection; physical devices are ignored in this mode.
use sdl2::{controller::{Axis,Button},sys};
pub(super) struct TestPad {index:i32,raw:*mut sys::SDL_Joystick,pub instance:u32}
impl TestPad {
 pub fn attach()->Result<Self,String>{unsafe{
  let index=sys::SDL_JoystickAttachVirtual(sys::SDL_JoystickType::SDL_JOYSTICK_TYPE_GAMECONTROLLER,6,21,0);
  if index<0{return Err(sdl2::get_error());}
  let raw=sys::SDL_JoystickOpen(index);
  if raw.is_null(){sys::SDL_JoystickDetachVirtual(index);return Err(sdl2::get_error());}
  Ok(Self{index,raw,instance:sys::SDL_JoystickInstanceID(raw) as u32})
 }}
 pub fn button(&mut self,name:&str,down:bool)->Result<(),String>{
  let b=match name {"A"=>Button::A,"B"=>Button::B,"X"=>Button::X,"Y"=>Button::Y,
  "LB"=>Button::LeftShoulder,"RB"=>Button::RightShoulder,"START"=>Button::Start,"BACK"=>Button::Back,
  "LS"=>Button::LeftStick,"RS"=>Button::RightStick,"UP"=>Button::DPadUp,"RIGHT"=>Button::DPadRight,
  "DOWN"=>Button::DPadDown,"LEFT"=>Button::DPadLeft,_=>return Err("unsupported test pad button".into())};
  let result=unsafe{sys::SDL_JoystickSetVirtualButton(self.raw,b as i32,u8::from(down))};
  if result<0{return Err(sdl2::get_error());}unsafe{sys::SDL_JoystickUpdate();}Ok(())
 }
 pub fn axis(&mut self,name:&str,value:i16)->Result<(),String>{
  let a=match name {"LX"=>Axis::LeftX,"LY"=>Axis::LeftY,"RX"=>Axis::RightX,"RY"=>Axis::RightY,"LT"=>Axis::TriggerLeft,"RT"=>Axis::TriggerRight,_=>return Err("unsupported test pad axis".into())};
  if unsafe{sys::SDL_JoystickSetVirtualAxis(self.raw,a as i32,value)}<0{return Err(sdl2::get_error());}
  unsafe{sys::SDL_JoystickUpdate();}Ok(())
 }
}
impl Drop for TestPad {fn drop(&mut self){unsafe{sys::SDL_JoystickClose(self.raw);sys::SDL_JoystickDetachVirtual(self.index);}}}
