package org.touchhle.android;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.zip.*;

public final class SaveRoundtripTest {
    static void check(boolean value,String message){if(!value)throw new AssertionError(message);}
    static Map<String,byte[]> snapshot(File root)throws Exception {
        Map<String,byte[]> result=new TreeMap<>();
        try(java.util.stream.Stream<Path> stream=Files.walk(root.toPath())) {
            for(Path p:(Iterable<Path>)stream.filter(Files::isRegularFile)::iterator)result.put(root.toPath().relativize(p).toString().replace('\\','/'),Files.readAllBytes(p));
        }return result;
    }
    static void same(Map<String,byte[]> a,Map<String,byte[]> b){check(a.keySet().equals(b.keySet()),"File set changed");for(String n:a.keySet())check(Arrays.equals(a.get(n),b.get(n)),"Unexpected change: "+n);}
    public static void main(String[] args)throws Exception {
        File source=new File(args[0]),base=new File(args[1]);check(!base.exists(),"Test destination exists");
        File docs=SaveArchiveCore.documents(base);check(docs.mkdirs(),"Cannot create test root");
        Map<String,byte[]> originals=snapshot(source);
        for(Map.Entry<String,byte[]> e:originals.entrySet()){File p=new File(docs,e.getKey());p.getParentFile().mkdirs();Files.write(p.toPath(),e.getValue());}
        for(int i=1;i<3;i++)for(String suffix:new String[]{"CheckpointSave","LevelSave"}) {
            Files.copy(new File(docs,"var/00_"+suffix+".sb").toPath(),new File(docs,String.format("var/%02d_%s.sb",i,suffix)).toPath());
        }
        check(new File(docs,"loggedIn").createNewFile(),"Empty dependency test fixture already exists");
        Map<String,byte[]> before=snapshot(docs);
        SaveScalarEditor.Slot initial=SaveScalarEditor.read(base,1);
        File backup=SaveScalarEditor.save(base,1,95275,21,1);check(backup.isFile(),"No automatic backup");
        SaveScalarEditor.Slot changed=SaveScalarEditor.read(base,1);check(changed.credits==95275&&changed.nodes==21&&changed.rig==1,"Scalar edit failed");
        Map<String,byte[]> edited=snapshot(docs);
        for(String n:before.keySet())if(!n.equals("var/01_CheckpointSave.sb")&&!n.equals("var/01_LevelSave.sb"))check(Arrays.equals(before.get(n),edited.get(n)),"Other slot/dependency changed: "+n);
        for(String suffix:new String[]{"CheckpointSave","LevelSave"}) {
            String name="var/01_"+suffix+".sb";SaveScalarEditor parser=new SaveScalarEditor(new File(docs,name));
            File reference=new File(base,"reference-"+suffix+".sb");Files.write(reference.toPath(),before.get(name));SaveScalarEditor original=new SaveScalarEditor(reference);
            byte[] reversed=parser.edit(original.get(SaveScalarEditor.CREDITS),original.get(SaveScalarEditor.NODES),(int)original.get(SaveScalarEditor.RIG));check(Arrays.equals(reversed,before.get(name)),"Scalar reversal not byte-exact");
        }
        ByteArrayOutputStream output=new ByteArrayOutputStream();SaveArchiveCore.exportZip(base,output);
        Set<String> names=new HashSet<>();try(ZipInputStream zip=new ZipInputStream(new ByteArrayInputStream(output.toByteArray()))){ZipEntry e;while((e=zip.getNextEntry())!=null)names.add(e.getName());}
        for(int i=0;i<3;i++)check(names.contains(String.format("Documents/var/%02d_CheckpointSave.sb",i)),"ZIP missing slot "+i);
        File restored=new File(base.getParentFile(),base.getName()+"-restored");SaveArchiveCore.importZip(new ByteArrayInputStream(output.toByteArray()),restored);
        same(edited,snapshot(SaveArchiveCore.documents(restored)));
        try{SaveScalarEditor.save(base,1,10,10,8);throw new AssertionError("Out-of-range armor accepted");}catch(IOException expected){}same(edited,snapshot(docs));
        byte[] bad=Files.readAllBytes(new File(docs,"var/01_CheckpointSave.sb").toPath());bad[bad.length-8]^=1;
        File corrupt=new File(base,"corrupt.sb");Files.write(corrupt.toPath(),bad);
        try{new SaveScalarEditor(corrupt);throw new AssertionError("Corruption accepted");}catch(IOException expected){}
        ByteArrayOutputStream unsafe=new ByteArrayOutputStream();try(ZipOutputStream zip=new ZipOutputStream(unsafe)){zip.putNextEntry(new ZipEntry("../bad.sb"));zip.write(bad);zip.closeEntry();}
        try{SaveArchiveCore.importZip(new ByteArrayInputStream(unsafe.toByteArray()),base);throw new AssertionError("Unsafe ZIP accepted");}catch(IOException expected){}same(edited,snapshot(docs));
        ByteArrayOutputStream emptySave=new ByteArrayOutputStream();try(ZipOutputStream zip=new ZipOutputStream(emptySave)) {
            zip.putNextEntry(new ZipEntry("Documents/var/00_CheckpointSave.sb"));zip.write(before.get("var/00_CheckpointSave.sb"));zip.closeEntry();
            zip.putNextEntry(new ZipEntry("Documents/var/01_CheckpointSave.sb"));zip.closeEntry();
        }
        try{SaveArchiveCore.importZip(new ByteArrayInputStream(emptySave.toByteArray()),base);throw new AssertionError("Empty SBIN accepted");}catch(IOException expected){}same(edited,snapshot(docs));
        File masked=new File(base,"masked-profile"),maskedDocs=SaveArchiveCore.documents(masked);check(maskedDocs.mkdirs(),"Cannot create masked test root");
        for(Map.Entry<String,byte[]> e:before.entrySet()){File p=new File(maskedDocs,e.getKey());p.getParentFile().mkdirs();Files.write(p.toPath(),e.getValue());}
        SaveScalarEditor.save(masked,0,0,0,2,4);
        Map<String,byte[]> maskedAfter=snapshot(maskedDocs);
        for(String name:before.keySet()) {
            if(name.equals("var/00_CheckpointSave.sb")||name.equals("var/00_LevelSave.sb")) {
                File reference=new File(base,"mask-reference-"+new File(name).getName());Files.write(reference.toPath(),before.get(name));
                SaveScalarEditor old=new SaveScalarEditor(reference),changedFile=new SaveScalarEditor(new File(maskedDocs,name));
                check(changedFile.get(SaveScalarEditor.CREDITS)==old.get(SaveScalarEditor.CREDITS)&&changedFile.get(SaveScalarEditor.NODES)==old.get(SaveScalarEditor.NODES),"Armor-only edit changed another scalar");
                check(Arrays.equals(changedFile.edit(old.get(SaveScalarEditor.CREDITS),old.get(SaveScalarEditor.NODES),(int)old.get(SaveScalarEditor.RIG)),before.get(name)),"Armor-only reverse changed bytes");
            } else check(Arrays.equals(before.get(name),maskedAfter.get(name)),"Armor-only edit changed another file");
        }
        try{SaveScalarEditor.save(masked,0,0,0,0,0);throw new AssertionError("Empty edit accepted");}catch(IOException expected){}same(maskedAfter,snapshot(maskedDocs));
        same(originals,snapshot(source));
        System.out.println("PASS: three-slot ZIP roundtrip; selected-slot edit; armor-only edit preserves per-file nodes/credits; other slots/dependencies unchanged; byte-exact scalar reversal; automatic backup; range/corrupt/unsafe refusal; source untouched");
    }
}
