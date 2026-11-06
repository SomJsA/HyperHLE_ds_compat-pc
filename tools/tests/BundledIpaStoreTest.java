package org.touchhle.android;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
public final class BundledIpaStoreTest {
    static void check(boolean test,String message){if(!test)throw new AssertionError(message);}
    static void write(File file,byte[] data)throws Exception{file.getParentFile().mkdirs();Files.write(file.toPath(),data);}
    public static void main(String[] args)throws Exception {
        File root=new File(args[0]);check(!root.exists(),"Test scope exists");root.mkdirs();
        byte[] trusted="unchanged bundled IPA byte fixture".getBytes(StandardCharsets.UTF_8),original="user's different original IPA".getBytes(StandardCharsets.UTF_8);
        File reference=new File(root,"reference.ipa");write(reference,trusted);String hash=BundledIpaStore.hash(reference);
        File base=new File(root,"unmarked"),legacy=new File(base,"touchHLE_apps/DeadSpace.ipa"),marker=new File(base,"touchHLE_apps/.DeadSpace.ipa.bundle-id");write(legacy,original);write(marker,"older-marker".getBytes(StandardCharsets.UTF_8));
        final int[] opened={0};File managed=BundledIpaStore.ensure(base,hash,()->{opened[0]++;return new ByteArrayInputStream(trusted);});
        check(!legacy.equals(managed)&&Arrays.equals(original,Files.readAllBytes(legacy.toPath())),"Original IPA overwritten");
        check(new String(Files.readAllBytes(marker.toPath()),StandardCharsets.UTF_8).equals("older-marker"),"Old marker overwritten");
        check(Arrays.equals(trusted,Files.readAllBytes(managed.toPath()))&&opened[0]==1,"Managed copy differs");
        check(BundledIpaStore.ensure(base,hash,()->{throw new IOException("Unexpected re-copy");}).equals(managed),"Cache not reused");
        File marked=new File(root,"marked"),markedLegacy=new File(marked,"touchHLE_apps/DeadSpace.ipa");write(markedLegacy,original);write(new File(marked,"touchHLE_apps/.DeadSpace.ipa.bundle-id"),hash.getBytes(StandardCharsets.UTF_8));
        check(BundledIpaStore.ensure(marked,hash,()->{throw new IOException("Marked input should be retained");}).equals(markedLegacy),"Marked legacy input changed");
        File corruptBase=new File(root,"bad-source");
        try{BundledIpaStore.ensure(corruptBase,hash,()->new ByteArrayInputStream(original));throw new AssertionError("Bad asset accepted");}catch(IOException expected){}
        check(!new File(corruptBase,"touchHLE_bundled_cache/"+hash).exists(),"Bad asset published");
        check(new File(corruptBase,"touchHLE_bundled_cache").list().length==0,"Owned partial left behind");
        write(managed,original);
        try{BundledIpaStore.ensure(base,hash,()->{throw new AssertionError("Unknown cached input must not be overwritten");});throw new AssertionError("Unknown cache accepted");}catch(IOException expected){}
        check(Arrays.equals(original,Files.readAllBytes(managed.toPath())),"Unknown cached data overwritten");
        try{BundledIpaStore.ensure(base,"../bad",()->new ByteArrayInputStream(trusted));throw new AssertionError("Unsafe cache key accepted");}catch(IOException expected){}
        System.out.println("PASS: unknown original IPA/marker retained; marked legacy retained; exact managed copy; idempotent reuse; bad asset/cache/path refused without overwrites; owned partial cleanup");
    }
}
