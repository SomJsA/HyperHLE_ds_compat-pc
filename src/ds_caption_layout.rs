//! Cached-caption planning. Page selection uses the native cue clock.
pub(crate) struct Plan { pub size:f32, pub pages:Vec<String> }
fn closing(c:char)->bool {"，。！？；：、…）》」』】,.!?;:)".contains(c)}
fn units(text:&str)->Vec<String>{
    let mut units:Vec<String>=Vec::new();
    for c in text.chars(){
        if closing(c)&&units.last().is_some_and(|s|s!="\n"){units.last_mut().unwrap().push(c);}
        else{units.push(c.to_string());}
    }
    units
}
pub(crate) fn wrap(text:&str,width:f32,measure:impl Fn(&str)->f32)->Vec<String>{
    let mut out=Vec::new();let mut line=String::new();
    for unit in units(text) {
        if unit=="\n"{out.push(std::mem::take(&mut line));continue;}
        if !line.is_empty()&&measure(&format!("{line}{unit}"))>width {out.push(std::mem::take(&mut line));}
        line.push_str(&unit);
    }
    if !line.is_empty()||out.is_empty(){out.push(line);}out
}
pub(crate) fn plan(text:&str,width:f32,requested:f32,max_height:f32,line_height:f32,paged:bool,measure:impl Fn(f32,&str)->f32)->Plan{
    let max_lines=if paged{2}else{3};
    let mut size=requested.min(max_height/(max_lines as f32*line_height)).max(1.);
    let count=|s:&str,size:f32|wrap(s,width,|t|measure(size,t)).len();
    if !paged {
        let(mut lo,mut hi)=(1f32,size);
        for _ in 0..16 {let mid=(lo+hi)*0.5;if count(text,mid)<=3{lo=mid;}else{hi=mid;}}
        size=lo;return Plan{size,pages:vec![text.to_owned()]};
    }
    let mut clauses=Vec::new();let mut clause=String::new();
    for c in text.chars(){clause.push(c);if "。！？；，、".contains(c){clauses.push(std::mem::take(&mut clause));}}
    if !clause.is_empty(){clauses.push(clause);}
    let mut pages=Vec::new();let mut page=String::new();
    for clause in clauses {
        if count(&format!("{page}{clause}"),size)<=2 {page.push_str(&clause);continue;}
        if !page.is_empty(){pages.push(std::mem::take(&mut page));}
        for unit in units(&clause){
            if !page.is_empty()&&count(&format!("{page}{unit}"),size)>2 {pages.push(std::mem::take(&mut page));}
            page.push_str(&unit);
        }
    }
    if !page.is_empty()||pages.is_empty(){pages.push(page);}
    Plan{size,pages}
}
pub(crate) fn page_at(pages:&[String],elapsed:u32,total:u32)->usize{
    if total==0||pages.len()<2{return 0;}
    let weight=|s:&String|s.chars().count() as u64+5;
    let all=pages.iter().map(weight).sum::<u64>();let target=u64::from(elapsed.min(total))*all;
    let mut end=0;
    for (i,p) in pages.iter().enumerate(){end+=weight(p);if target<u64::from(total)*end{return i;}}
    pages.len()-1
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn real_long_cue_is_lossless_and_bounded(){
        let text="破坏者，我代表教会。听着，你眼前所见，就是对你今天功劳的奖赏。你有幸成为第一批荣获眷顾的人。当局正在封锁泰坦空间站，切断矿区通道……把你关在里面。但别害怕。你的死将无比光荣，带你踏上神圣的往生之路，通往教会许诺的死后新生。赞美阿尔特曼。";
        let p=plan(text,30.,1.,3.,1.,true,|size,t|t.chars().count() as f32*size);
        assert_eq!(p.pages.concat(),text);assert!(p.pages.len()>1);
        assert!(p.pages.iter().all(|t|wrap(t,30.,|s|s.chars().count() as f32*p.size).len()<=2));
        let p=plan(text,300.,10.,30.,1.,false,|size,t|t.chars().count() as f32*size);
        assert_eq!(p.pages,vec![text]);assert!(wrap(text,300.,|s|s.chars().count() as f32*p.size).len()<=3);
    }
    #[test]fn native_clock_selects_pages_without_a_host_timer(){
        let pages=vec!["第一段".into(),"第二段".into()];
        assert_eq!(page_at(&pages,0,10_000),0);assert_eq!(page_at(&pages,4999,10_000),0);
        assert_eq!(page_at(&pages,5000,10_000),1);assert_eq!(page_at(&pages,10_000,10_000),1);
        assert_eq!(page_at(&pages,0,0),0);
    }
    #[test]fn punctuation_does_not_start_a_new_line(){
        let lines=wrap("注意，前方。",3.,|s|s.chars().count() as f32);
        assert_eq!(lines.concat(),"注意，前方。");assert!(lines.iter().all(|s|!s.starts_with(['，','。'])));
    }
}
