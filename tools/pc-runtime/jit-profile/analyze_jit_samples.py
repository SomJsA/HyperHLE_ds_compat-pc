"""Join finite wall-clock IP observations to timestamped emitted-code ranges.

Counts describe captured instruction pointers, including waits/preemption. They
are not exclusive CPU time or on-CPU percentages. Cache clears and ambiguous
capture intervals deliberately reduce coverage instead of using stale symbols.
"""
import argparse
from bisect import bisect_left, bisect_right
from collections import Counter
import json
from pathlib import Path
import re
import statistics

class Ranges:
    def __init__(self):
        self.rows=[]
        self.starts=[]

    def put(self, lo, hi, name):
        if hi<=lo:
            raise ValueError('Invalid emitted range')
        if not self.rows or lo>=self.rows[-1][1]:
            self.rows.append((lo,hi,name));self.starts.append(lo);return
        begin=bisect_left(self.starts,lo)
        if begin and self.rows[begin-1][1]>lo:
            begin-=1
        end=begin
        while end<len(self.rows) and self.rows[end][0]<hi:
            end+=1
        replacement=[]
        if begin<end and self.rows[begin][0]<lo:
            old=self.rows[begin];replacement.append((old[0],lo,old[2]))
        replacement.append((lo,hi,name))
        if begin<end and self.rows[end-1][1]>hi:
            old=self.rows[end-1];replacement.append((hi,old[1],old[2]))
        self.rows[begin:end]=replacement
        self.starts[begin:end]=[row[0] for row in replacement]

    def find(self, address):
        index=bisect_right(self.starts,address)-1
        if index>=0 and address<self.rows[index][1]:
            return self.rows[index][2]
        return None

def rows(path, maximum):
    if path.stat().st_size>maximum:
        raise ValueError('Input exceeds bounded diagnostic contract')
    with path.open(encoding='utf-8') as stream:
        for line in stream:
            if not line.endswith('\n'):
                raise ValueError('Incomplete diagnostic record')
            yield line.rstrip('\n').split('\t')

def join(map_path, samples_path):
    mapped=list(rows(map_path,16*1024*1024))
    captured=list(rows(samples_path,8*1024*1024))
    if not mapped or mapped[0][:2]!=['H','2'] or len(mapped[0])!=5:
        raise ValueError('Unknown map header')
    if not captured or captured[0][:2]!=['H','1'] or len(captured[0])!=6:
        raise ValueError('Unknown sampler header')
    if (int(mapped[0][2])!=int(captured[0][2]) or int(mapped[0][3])!=int(captured[0][4])
            or int(mapped[0][4])!=int(captured[0][5])):
        raise ValueError('PID, creation time or QPC identity mismatch')
    if int(captured[0][5])<=0 or int(captured[0][4])<=0:
        raise ValueError('Invalid process identity/timebase')
    events=[];epoch=0;previous=0;limit=None
    for row in mapped[1:]:
        if limit is not None:
            raise ValueError('Records after map limit')
        at=int(row[1])
        if at<previous:
            raise ValueError('Map time moved backwards')
        previous=at
        if row[0]=='M' and len(row)==6 and int(row[2])==epoch:
            lo=int(row[3],16);hi=lo+int(row[4],16)
            if not 0<lo<hi<=2**64:
                raise ValueError('Invalid emitted extent')
            events.append((at,'M',lo,hi,row[5]))
        elif row[0]=='C' and len(row)==3 and int(row[2])==epoch+1:
            epoch+=1;events.append((at,'C',0,0,''))
        elif row[0]=='L' and len(row)==4 and int(row[2])==epoch:
            limit=at;events.append((at,'L',0,0,''))
        else:
            raise ValueError('Unknown map record or inconsistent epoch')
    if captured[-1][0]!='F' or len(captured[-1])!=5 or int(captured[-1][4])!=0:
        raise ValueError('Sampler incomplete or cleanup failed')
    modules=Ranges();current=Ranges();event_index=0
    counts=Counter();blocks=Counter();categories=Counter();costs=[]
    samples=0;missing=0;previous_end=0;limited=False
    for row in captured[1:-1]:
        if row[0]=='B' and len(row)==5:
            modules.put(int(row[2],16),int(row[2],16)+int(row[3],16),row[4]);continue
        if row[0]=='U' and len(row)==4:
            missing+=1;continue
        if row[0]!='S' or len(row)!=8:
            raise ValueError('Sampler contains failure/unknown record')
        begin,context,end=[int(value) for value in row[1:4]]
        if not previous_end<=begin<=context<=end:
            raise ValueError('Invalid sample clock order')
        previous_end=end
        rip=int(row[4],16)
        while event_index<len(events) and events[event_index][0]<=begin:
            _,kind,lo,hi,name=events[event_index]
            if kind=='C':current=Ranges()
            elif kind=='M':current.put(lo,hi,name)
            else:limited=True
            event_index+=1
        ambiguous=False
        for future_index in range(event_index,len(events)):
            event=events[future_index]
            if event[0]>end:break
            if event[1] in ('C','L') or event[2]<=rip<event[3]:
                ambiguous=True;break
        name=current.find(rip) if not limited and not ambiguous else None
        if ambiguous:category='ambiguous_mapping_transition'
        elif limited:category='map_limit_reached'
        elif name:
            category='jit';counts[name]+=1
            match=re.fullmatch(r'a32_([at])([0-9A-F]{8})_(le|be)_fpcr[0-9A-F]{8}',name)
            if match:blocks['0x'+match[2]]+=1
        else:
            module=modules.find(rip)
            category='module:'+module if module else 'unknown_native_or_jit'
        categories[category]+=1;samples+=1
        costs.append((end-begin)*1e6/int(captured[0][4]))
    if samples!=int(captured[-1][2]) or missing!=int(captured[-1][3]):
        raise ValueError('Sampler footer counts mismatch')
    if int(captured[-1][1])<previous_end:
        raise ValueError('Sampler footer precedes captured contexts')
    if not samples:
        raise ValueError('No captured instruction pointers')
    def top(counter):
        return [dict(name=name,samples=count,all_wall_sample_percent=100*count/samples)
                for name,count in counter.most_common(30)]
    return dict(schema=1,pid=int(captured[0][2]),main_tid=int(captured[0][3]),
        process_created_100ns=int(captured[0][5]),qpc_frequency=int(captured[0][4]),
        samples=samples,missing_contexts=missing,map_records=len(events),cache_clear_epochs=epoch,
        map_limited=limit is not None,categories=dict(categories),top_jit_labels=top(counts),
        top_arm_block_addresses=top(blocks),
        arm_block_histogram=dict(blocks),jit_label_histogram=dict(counts),
        capture_wall_us=dict(median=statistics.median(costs),p95=sorted(costs)[int(.95*(len(costs)-1))],maximum=max(costs)),
        scope='Wall-clock saved-IP observations of earliest-created owned thread, including waits/preemption and PSS capture perturbation; not exclusive CPU or on-CPU shares. Unknowns, epoch changes and truncation retained. Native module names use inventories taken by the observer; loader history is not captured. No FPS claim.')

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--map',type=Path,required=True)
    parser.add_argument('--samples',type=Path,required=True)
    parser.add_argument('--native-log',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    with args.native_log.open(encoding='utf-8',errors='replace') as stream:
        if any('[DSjit-map] disabled' in line and 'reason=limit ' not in line for line in stream):
            raise ValueError('Native map disabled after an I/O/configuration error')
    result=join(args.map,args.samples)
    with args.output.open('x',encoding='utf-8') as stream:
        json.dump(result,stream,indent=2);stream.write('\n')
    print(json.dumps({key:result[key] for key in ('samples','missing_contexts','map_records','cache_clear_epochs','categories','top_arm_block_addresses','capture_wall_us')}))

if __name__=='__main__':main()
