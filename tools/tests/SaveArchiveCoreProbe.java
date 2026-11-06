package org.touchhle.android;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import java.util.zip.ZipOutputStream;

public final class SaveArchiveCoreProbe {
    public static void main(String[] args) throws Exception {
        verifyImportAndBackup();
        verifyRejectWithoutSave();
        System.out.println("save_import_probe=ok");
    }

    private static void verifyImportAndBackup() throws Exception {
        File base = new File(System.getProperty("java.io.tmpdir"),
                "deadspace-save-import-probe-" + System.nanoTime());
        File documents = new File(base,
                "touchHLE_sandbox/com.ea.deadspace.bv/Documents");
        File oldSave = new File(documents, "var/old.sb");
        require(oldSave.getParentFile().mkdirs(), "create old save directory");
        try (FileOutputStream output = new FileOutputStream(oldSave)) {
            output.write("old-save".getBytes(StandardCharsets.UTF_8));
        }

        byte[] zip = zip("Documents/var/new.sb", "new-save");
        SaveArchiveCore.Result result = SaveArchiveCore.importZip(
                new ByteArrayInputStream(zip), base);
        require(result.importedFiles == 1, "one imported save");
        require(new File(documents, "var/new.sb").isFile(), "new save exists");
        require(!new File(documents, "var/old.sb").exists(), "old save replaced");
        require(result.backup != null && result.backup.isFile(), "backup exists");
        require(contains(result.backup, "var/old.sb"), "backup contains old save");
        deleteTree(base);
    }

    private static void verifyRejectWithoutSave() throws Exception {
        File base = new File(System.getProperty("java.io.tmpdir"),
                "deadspace-save-import-reject-" + System.nanoTime());
        try {
            SaveArchiveCore.importZip(
                    new ByteArrayInputStream(zip("Documents/readme.txt", "not-a-save")), base);
            throw new AssertionError("archive without .sb was accepted");
        } catch (java.io.IOException expected) {
            require(expected.getMessage().contains("no .sb"), "rejection reason");
        } finally {
            deleteTree(base);
        }
    }

    private static byte[] zip(String name, String body) throws Exception {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        try (ZipOutputStream zip = new ZipOutputStream(bytes)) {
            zip.putNextEntry(new ZipEntry(name));
            zip.write(body.getBytes(StandardCharsets.UTF_8));
            zip.closeEntry();
        }
        return bytes.toByteArray();
    }

    private static boolean contains(File zipFile, String name) throws Exception {
        try (ZipInputStream zip = new ZipInputStream(new FileInputStream(zipFile))) {
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null) {
                if (name.equals(entry.getName())) {
                    return true;
                }
            }
        }
        return false;
    }

    private static void require(boolean condition, String label) {
        if (!condition) {
            throw new AssertionError(label);
        }
    }

    private static void deleteTree(File file) {
        if (file == null || !file.exists()) {
            return;
        }
        File[] children = file.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteTree(child);
            }
        }
        file.delete();
    }
}
