#!/usr/bin/env python3
"""Pack tutorial controls from the same local CC0 Kenney archive as menu hints."""
import argparse,hashlib,io,json,string
from pathlib import Path
from zipfile import ZipFile
from PIL import Image
p=argparse.ArgumentParser();p.add_argument('--pack',type=Path,required=True);args=p.parse_args()
out=Path(__file__).parent/'assets/input-prompts';pack=ZipFile(args.pack);names=set(pack.namelist())
selected={}
keys=list(string.ascii_lowercase+string.digits)+['space','shift','enter','escape','tab','ctrl','alt','backspace','delete','insert','home','end','page_up','page_down','capslock']+['f'+str(i)for i in range(1,13)]+['arrow_'+s for s in ['up','down','left','right']]
for key in keys:
 name=f'Keyboard & Mouse/Double/keyboard_{key}.png'
 if name in names:selected['key/'+key]=name
for key in ['a','b','x','y']:selected['pad/'+key]=f'Xbox Series/Double/xbox_button_color_{key}_outline.png'
for key in ['lb','rb','lt','rt']:selected['pad/'+key]=f'Xbox Series/Double/xbox_{key}_outline.png'
for key,file in [('start','button_start_outline'),('back','button_back_outline'),('ls','stick_l_press'),('rs','stick_r_press'),('left-stick','stick_l'),('right-stick','stick_r'),('dpad','dpad_none')]:selected['pad/'+key]=f'Xbox Series/Double/xbox_{file}.png'
for key in ['left','right','move']:selected['mouse/'+key]=f'Keyboard & Mouse/Double/mouse_{key}.png'
canvas=Image.new('RGBA',(1024,2048));rects={};x=y=8;row=0
for key,name in selected.items():
 image=Image.open(io.BytesIO(pack.read(name))).convert('RGBA');image=image.crop(image.getbbox())
 if x+image.width+8>canvas.width:x=8;y+=row+8;row=0
 assert y+image.height+8<=canvas.height
 canvas.alpha_composite(image,(x,y));rects[key]=[x,y,image.width,image.height]
 x+=image.width+8;row=max(row,image.height)
height=1
while height<y+row+8:height*=2
canvas=canvas.crop((0,0,1024,height));canvas.save(out/'tutorial-atlas.png',optimize=True)
(out/'tutorial-atlas.json').write_text(json.dumps({'size':list(canvas.size),'sprites':rects,'source':'https://kenney.nl/assets/input-prompts','source_pack_sha256':hashlib.sha256(args.pack.read_bytes()).hexdigest(),'selected_source_files':selected},indent=2)+'\n',encoding='utf-8')
preview=Image.new('RGB',(1200,180),(5,17,23));x=20
for key in ['key/w','key/a','key/s','key/d','pad/left-stick','key/space','pad/rb','pad/rt','pad/ls','pad/rs']:
 a,b,w,h=rects[key];im=canvas.crop((a,b,a+w,b+h));im.thumbnail((150,78),Image.Resampling.LANCZOS);preview.paste(im,(x,50),im);x+=im.width+16
preview.save(out/'tutorial-preview.jpg',quality=88)
print(json.dumps({'sprites':len(rects),'dimensions':list(canvas.size),'bytes':(out/'tutorial-atlas.png').stat().st_size}))
