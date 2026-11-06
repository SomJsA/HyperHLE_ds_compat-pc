#!/usr/bin/env python3
"""Pack selected Kenney Input Prompts sprites; no assets are downloaded here."""
import argparse
import hashlib
import io
import json
from pathlib import Path
from zipfile import ZipFile
from PIL import Image, ImageDraw, ImageFont

p = argparse.ArgumentParser()
p.add_argument('--pack', type=Path, required=True)
p.add_argument('--font', type=Path, required=True)
args = p.parse_args()
out = Path(__file__).parent / 'assets/input-prompts'
out.mkdir(parents=True, exist_ok=True)
pack = ZipFile(args.pack)
used = []

def sprite(name):
    used.append(name)
    image = Image.open(io.BytesIO(pack.read(name))).convert('RGBA')
    return image.crop(image.getbbox())

keyboard = lambda name: sprite(f'Keyboard & Mouse/Double/keyboard_{name}.png')
pad = lambda name: sprite(f'Xbox Series/Double/xbox_{name}.png')
atlas = Image.new('RGBA', (512, 384))
entries = {}

def put(name, image, x, y):
    atlas.alpha_composite(image, (x, y))
    entries[name] = [x, y, image.width, image.height]

put('E', keyboard('e'), 8, 8)
put('Q', keyboard('q'), 120, 8)
put('Enter', keyboard('enter'), 232, 8)
put('A', pad('button_color_a_outline'), 344, 8)
put('X', pad('button_color_x_outline'), 8, 120)
put('Delete', keyboard('delete'), 8, 264)
put('Start', pad('button_start_outline'), 120, 264)
arrows = Image.new('RGBA', (192, 128))
for name, xy in [('up',(64,0)),('left',(0,64)),('down',(64,64)),('right',(128,64))]:
    arrows.alpha_composite(keyboard('arrow_'+name).resize((60,60),Image.Resampling.LANCZOS), xy)
put('Arrows', arrows, 120, 120)
put('Dpad', pad('dpad_none'), 320, 120)
font = ImageFont.truetype(str(args.font), 42)
label = Image.new('RGBA',(88,60))
draw = ImageDraw.Draw(label)
draw.text((2,0),'选择',font=font,fill=(183,237,243,255),stroke_width=1,stroke_fill=(3,15,20,230))
put('Select', label, 420, 142)
for name,text,x,w in [('Press','按',232,40),('Continue','以继续',280,168)]:
    im=Image.new('RGBA',(w,60));draw=ImageDraw.Draw(im)
    draw.text((0,0),text,font=font,fill=(183,237,243,255),stroke_width=1,stroke_fill=(3,15,20,230))
    put(name,im,x,264)

atlas.save(out/'atlas.png',optimize=True)
(out/'License-Kenney.txt').write_bytes(pack.read('License.txt'))
(out/'atlas.json').write_text(json.dumps({'size':[512,384],'sprites':entries,
    'source':'https://kenney.nl/assets/input-prompts',
    'source_pack_sha256':hashlib.sha256(args.pack.read_bytes()).hexdigest(),
    'selected_source_files':used,'label_font':args.font.name},indent=2)+'\n',encoding='utf-8')

preview=Image.new('RGB',(1280,170),(5,17,23))
canvas=ImageDraw.Draw(preview)
f=ImageFont.truetype(str(args.font),24)
for title,names,x in [('确认',['E','Enter','A'],26),('返回',['Q','X'],340),('选择',['Arrows','Dpad','Select'],600),('删除',['Delete','Start'],1000)]:
    canvas.text((x,18),title,font=f,fill=(158,208,217))
    pos=x
    for name in names:
        bx,by,bw,bh=entries[name]
        im=atlas.crop((bx,by,bx+bw,by+bh))
        h=48; w=round(im.width*h/im.height)
        im=im.resize((w,h),Image.Resampling.LANCZOS)
        preview.paste(im,(pos,72),im);pos+=w+12
preview.save(out/'preview.png',optimize=True)
print(json.dumps({'atlas_bytes':(out/'atlas.png').stat().st_size,'preview_bytes':(out/'preview.png').stat().st_size,'sprites':entries}))
