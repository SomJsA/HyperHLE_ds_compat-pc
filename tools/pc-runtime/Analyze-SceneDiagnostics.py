"""Offline summary of exported/current PC diagnostic sessions; never starts game.

Bounded worst-frame set and 0.5ms histogram. Raw original logs remain untouched.
Run after play, with --session repeated for each desired session directory.
"""
from pathlib import Path
import argparse,collections,csv,heapq,json,math,struct

def summarize(root):
    root=Path(root)
    if root.is_symlink() or not root.is_dir():raise ValueError('A normal session directory is required')
    bins=[0]*1002;count=total=maximum=over50=0;worst=[];first=None;last=None;missing=0;duplicates=0
    byphase=collections.Counter()
    for path in sorted(root.glob('frames-*.csv')):
        if path.is_symlink():raise ValueError('Symlink input refused')
        with path.open(encoding='utf-8-sig',newline='') as stream:
            for row in csv.DictReader(stream):
                values=[int(row[k]) for k in ['mono_us','native_present','interval_us','phase','native_state','native_hud']]
                at,n,dt,phase,state,hud=values
                if min(at,n,dt,phase)<0:raise ValueError('Invalid negative frame field')
                if first is None:first=n
                if last is not None:
                    if n>last+1:missing+=n-last-1
                    if n<=last:duplicates+=1
                last=n;byphase[phase]+=1
                if phase!=4:continue
                count+=1;total+=dt;maximum=max(maximum,dt);over50+=dt>50000;bins[min(dt//500,1001)]+=1
                item=(dt,at,n,state,hud)
                if len(worst)<32:heapq.heappush(worst,item)
                elif item>worst[0]:heapq.heapreplace(worst,item)
    def percentile(p):
        target=math.ceil(count*p);seen=0
        for i,value in enumerate(bins):
            seen+=value
            if seen>=target and target:return maximum/1000 if i==1001 else (i+1)*.5
        return None
    windows=[{'frame':n,'mono_us':at,'interval_us':dt,'state':state,'hud':hud,'overlapping_spans':[],'nearby_events':[]} for dt,at,n,state,hud in sorted(worst,reverse=True)]
    kinds={};chapters=[];gaps=[];cost=secondary=0;seconds=0;activity={};bindings={};positions=0;keys=0;lost=skipped=0;origin_tags=set()
    startup=root/'session-start.json'
    if startup.exists():
        data=json.loads(startup.read_text(encoding='utf-8-sig'));kinds=data.get('scene_diagnostics',{}).get('kinds',[])
    for path in sorted(root.glob('timeline-*.jsonl')):
        if path.is_symlink():raise ValueError('Symlink input refused')
        with path.open(encoding='utf-8-sig') as stream:
            for line in stream:
                if len(line)>1024*1024:raise ValueError('Unexpected oversized timeline line')
                try:e=json.loads(line)
                except json.JSONDecodeError:continue # Partial final line is not a fabricated event.
                at=e.get('mono_us',0);code=e.get('code');values=e.get('values',[])
                if 'origin_thread_tag' in e:origin_tags.add(e['origin_thread_tag'])
                if e.get('type')=='one_second_essentials':
                    seconds+=1;cost+=e.get('collector_ns',0);secondary+=e.get('secondary_thread_collector_ns',0)
                    lost=max(lost,e.get('dropped_total',0));skipped=max(skipped,e.get('detail_skipped_total',0))
                    for name,v in (e.get('scene_activity') or {}).items():
                        dst=activity.setdefault(name,{'calls':0,'units':0,'inclusive_wall_us':0,'max_wall_us':0})
                        for field in ['calls','units','inclusive_wall_us']:dst[field]+=v.get(field,0)
                        dst['max_wall_us']=max(dst['max_wall_us'],v.get('max_wall_us',0))
                if e.get('type') in ['self_protection','detail_level_change','writer_stopped']:
                    if len(gaps)<256:gaps.append(e)
                if code=='native_chapter' and len(chapters)<256:chapters.append(e)
                if code=='probe_player_position_bits':positions+=1
                if code=='render_state_key_0':keys+=1
                if code=='hle_binding':
                    split=e.get('value','').split(':',2)
                    if len(split)==3 and len(bindings)<16384:bindings[split[0]]=split[2]
                for w in windows:
                    begin=w['mono_us']-w['interval_us'];end=w['mono_us']
                    if code in ['runtime_wall_span','scene_slow_scope'] and len(values)==4:
                        kind,us,frame,tag=values;overlap=min(at,end)-max(at-us,begin)
                        if overlap>0 and len(w['overlapping_spans'])<128:
                            label=kinds[kind] if code=='scene_slow_scope' and kind<len(kinds) else kind
                            w['overlapping_spans'].append({'observer':code,'kind':label,'wall_us':us,'overlap_us':overlap,'end_us':at,'tag':tag,'origin_thread_tag':e.get('origin_thread_tag')})
                    elif abs(at-end)<=2_000_000 and code in ['native_state','native_chapter','native_prompt_kind','render_state_key_0','hd_replacement_submission','hd_texture_submission']:
                        if len(w['nearby_events'])<32:w['nearby_events'].append(e)
    for w in windows:
        w['overlapping_spans'].sort(key=lambda v:v['overlap_us'],reverse=True)
        for span in w['overlapping_spans']:
            if span['observer']=='runtime_wall_span' and span['kind'] in (13,14):span['guest_import_hint']=bindings.get(f"{span['tag']:08x}")
    return {'schema':1,'session':root.name,'scope':'Offline recorded-detail summary; all raw frames retained including long intervals. Phase4 only for live stats. Missing/downgraded detail is not no-stutter proof. Inclusive spans overlap and may include cooperative scheduling; never sum as exclusive time. First-use partial render key is not shader-compilation proof.',
        'recorded_phase_counts':dict(byphase),'recorded_live_frames':count,'recorded_live_seconds':total/1e6,'recorded_live_fps':count*1e6/total if total else None,'p99_ms_upper_bin_estimate':percentile(.99),'max_ms':maximum/1000,'over50ms':over50,
        'first_recorded_native_frame':first,'last_recorded_native_frame':last,'missing_frame_numbers':missing,'duplicate_or_out_of_order_frames':duplicates,'dropped_messages_max':lost,'skipped_frames_max':skipped,
        'observed_chapter_events':chapters,'position_samples':positions,'partial_render_first_keys':keys,'producer_thread_tags':sorted(origin_tags),'scene_activity':activity,
        'collector_cost_ns_recorded_core':cost,'collector_cost_ns_recorded_secondary':secondary,'one_second_records':seconds,'detail_changes_first256':gaps,'worst32_recorded_live_frames':windows,
        'missing_observers':['GPU completion time and hidden driver compilation not directly observed','JIT code fetches are not compiler duration or executed instruction count','No generic logical quest-completion oracle']}

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--session',type=Path,action='append',required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    data={'schema':1,'sessions':[summarize(root) for root in args.session]}
    with args.output.open('x',encoding='utf-8') as stream:json.dump(data,stream,ensure_ascii=True,indent=2)
    print(json.dumps({'sessions':len(data['sessions']),'output_bytes':args.output.stat().st_size,'live_frames':sum(v['recorded_live_frames'] for v in data['sessions'])}))
