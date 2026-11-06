export const names={chapter:'章节标题',objective:'任务追踪',subtitle:'对白字幕'};
export const limits={x:[0,100],y:[0,100],width:[10,96],fontSize:[3,36],secondaryFontSize:[3,30],lineHeight:[1,2],padding:[0,20],radius:[0,20],opacity:[0,1],strokeOpacity:[0,1],gap:[0,15],maxHeight:[10,80]};
export function gameRect(w,h,aspect=1.5){const cw=Math.min(w,h*aspect),ch=cw/aspect;return{x:(w-cw)/2,y:(h-ch)/2,w:cw,h:ch};}
export function resolve(config,layer,id){return {...config.layers[layer],...(config.overrides?.[id]?.style||{})};}
export function validate(value){
 if(value?.schema!==1||!value.layers||typeof value.layers!=='object')throw Error('需要 schema=1 的图文配置');
 const out=structuredClone(value);out.overrides??={};
 for(const [name,l] of Object.entries(out.layers)){if(!names[name])throw Error('未知图层 '+name);validateStyle(l,false);}
 if(Object.keys(out.overrides).length>2000)throw Error('条目数量过多');
 for(const [id,e] of Object.entries(out.overrides)){if(!id||id.length>128||!e||typeof e!=='object')throw Error('条目 ID 无效');
  for(const key of ['text','secondaryText'])if(e[key]!==undefined&&(typeof e[key]!=='string'||!e[key].trim()||e[key].length>4000))throw Error(id+' 的文案无效');
  if(e.style)validateStyle(e.style,true);
 }return out;
}
function validateStyle(l,single){for(const [key,val] of Object.entries(l)){
 if(single&&key==='enabled')throw Error('单条样式不覆盖总开关');
 if(key in limits){let [min,max]=limits[key];if(typeof val!=='number'||!Number.isFinite(val)||val<min||val>max)throw Error('参数超出范围：'+key);}
 if(['color','secondaryColor','background','stroke'].includes(key)&&!/^#[\da-f]{6}$/i.test(val))throw Error('颜色需要 #RRGGBB');
 } }
export function wrap(ctx,text,width){
 const lines=[];for(const paragraph of String(text).split('\n')){let line='';for(const ch of paragraph){
  if(line&&ctx.measureText(line+ch).width>width){lines.push(line);line=ch.trimStart();}else line+=ch;
 }lines.push(line);}return lines;
}
export function card(ctx,style,first,second,rect,guide=false){
 if(!style.enabled||!first)return null;
 const sy=rect.h/320,pad=style.padding*sy,width=Math.min(rect.w-4*sy,rect.w*style.width/100),inner=Math.max(1,width-pad*2);
 const font=s=>`${s*sy}px "GameText",sans-serif`;
 ctx.font=font(style.fontSize);const lines=wrap(ctx,first,inner),line=style.fontSize*sy*style.lineHeight;
 const subSize=style.secondaryFontSize||8.5;ctx.font=font(subSize);const subs=second?wrap(ctx,second,inner):[],subLine=subSize*sy*style.lineHeight;
 const firstH=lines.length*line,height=2*pad+firstH+(subs.length?(style.gap||0)*sy+subs.length*subLine:0),scale=Math.min(1,rect.h*style.maxHeight/100/height);
 let x=rect.x+rect.w*style.x/100-width*scale/2,anchor=rect.y+rect.h*style.y/100;
 let y=anchor-(style.anchor==='bottom-center'?height*scale:style.anchor==='first-center'?(pad+firstH/2)*scale:height*scale/2);
 x=Math.max(rect.x,Math.min(x,rect.x+rect.w-width*scale));y=Math.max(rect.y,Math.min(y,rect.y+rect.h-height*scale));
 ctx.save();ctx.translate(x,y);ctx.scale(scale,scale);ctx.beginPath();ctx.roundRect(0,0,width,height,style.radius*sy);ctx.fillStyle=style.background;ctx.globalAlpha=style.opacity;ctx.fill();
 ctx.globalAlpha=style.strokeOpacity;ctx.strokeStyle=style.stroke;ctx.lineWidth=Math.max(1,sy*.35);ctx.stroke();ctx.globalAlpha=1;
 const align=style.align||'center';ctx.textAlign=align;ctx.textBaseline='middle';ctx.shadowColor='#000b';ctx.shadowBlur=sy*.5;ctx.shadowOffsetY=sy*.5;
 const tx=align==='left'?pad:align==='right'?width-pad:width/2;
 ctx.font=font(style.fontSize);ctx.fillStyle=style.color;lines.forEach((t,i)=>ctx.fillText(t,tx,pad+line*(i+.5)));
 ctx.font=font(subSize);ctx.fillStyle=style.secondaryColor||style.color;subs.forEach((t,i)=>ctx.fillText(t,tx,pad+firstH+(style.gap||0)*sy+subLine*(i+.5)));
 ctx.restore();const bounds={x,y,w:width*scale,h:height*scale};
 if(guide){ctx.save();ctx.strokeStyle='#7deaf1';ctx.lineWidth=1.5;ctx.setLineDash([5,4]);ctx.strokeRect(x-3,y-3,bounds.w+6,bounds.h+6);ctx.setLineDash([]);ctx.fillStyle='#85f1f4';ctx.fillRect(x+bounds.w-3,y+bounds.h-3,6,6);ctx.restore();}
 return bounds;
}
