package org.touchhle.android;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import java.util.zip.ZipOutputStream;

/**
 * Imports a user-selected ZIP archive into the Dead Space Documents sandbox.
 *
 * Accepted layouts are a Documents-prefixed tree, a package/Documents tree, or
 * a flat set of .sb files. Only .sb files are imported. Existing saves are
 * archived before replacement so a malformed import cannot silently erase the
 * last known-good set.
 */
final class SaveArchiveCore {
    static final long MAX_ARCHIVE_BYTES = 256L * 1024L * 1024L;
    private static final int MAX_ENTRIES = 10_000;
    private static final String PACKAGE_ID = "com.ea.deadspace.bv";

    static final class Result {
        final int importedFiles;
        final File backup;

        Result(int importedFiles, File backup) {
            this.importedFiles = importedFiles;
            this.backup = backup;
        }
    }

    private SaveArchiveCore() {}

    static Result importZip(InputStream input, File base) throws IOException {
        File packageRoot = new File(new File(base, "touchHLE_sandbox"), PACKAGE_ID);
        File documents = new File(packageRoot, "Documents");
        if (!packageRoot.isDirectory() && !packageRoot.mkdirs()) {
            throw new IOException("Could not create sandbox: " + packageRoot);
        }

        String stamp = new SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(new Date());
        File staging = new File(packageRoot, "Documents.import-" + stamp);
        File previous = new File(packageRoot, "Documents.previous-" + stamp);
        deleteTree(staging);
        if (!staging.mkdirs()) {
            throw new IOException("Could not create staging directory");
        }

        int importedFiles = 0;
        long importedBytes = 0;
        int entries = 0;
        try (ZipInputStream zip = new ZipInputStream(input)) {
            byte[] buffer = new byte[32 * 1024];
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null) {
                if (++entries > MAX_ENTRIES) {
                    throw new IOException("ZIP contains too many entries");
                }
                if (entry.isDirectory()) {
                    continue;
                }
                String relative = normalizeEntry(entry.getName());
                if (relative == null) {
                    continue;
                }
                File output = new File(staging, relative);
                String canonicalRoot = staging.getCanonicalPath() + File.separator;
                if (!output.getCanonicalPath().startsWith(canonicalRoot)) {
                    throw new IOException("Unsafe ZIP path: " + entry.getName());
                }
                File parent = output.getParentFile();
                if (parent == null || (!parent.isDirectory() && !parent.mkdirs())) {
                    throw new IOException("Could not create " + parent);
                }
                long entryBytes = 0;
                try (OutputStream file = new FileOutputStream(output)) {
                    int read;
                    while ((read = zip.read(buffer)) != -1) {
                        entryBytes += read;
                        importedBytes += read;
                        if (importedBytes > MAX_ARCHIVE_BYTES) {
                            throw new IOException("ZIP expands beyond 256 MB");
                        }
                        file.write(buffer, 0, read);
                    }
                }
                if (entryBytes == 0) {
                    output.delete();
                } else {
                    importedFiles++;
                }
            }
        } catch (IOException | RuntimeException error) {
            deleteTree(staging);
            throw error;
        }

        if (importedFiles == 0) {
            deleteTree(staging);
            throw new IOException("ZIP contains no .sb save files");
        }

        File backup = null;
        if (hasFiles(documents)) {
            File backupDir = new File(base, "save-backups");
            if (!backupDir.isDirectory() && !backupDir.mkdirs()) {
                throw new IOException("Could not create save backup directory");
            }
            backup = new File(backupDir, "Documents-" + stamp + ".zip");
            zipTree(documents, backup);
        }

        if (documents.exists() && !documents.renameTo(previous)) {
            deleteTree(staging);
            throw new IOException("Could not move the current save directory");
        }
        if (!staging.renameTo(documents)) {
            if (previous.exists()) {
                previous.renameTo(documents);
            }
            deleteTree(staging);
            throw new IOException("Could not activate imported saves");
        }
        deleteTree(previous);
        return new Result(importedFiles, backup);
    }

    private static String normalizeEntry(String raw) throws IOException {
        if (raw == null) {
            return null;
        }
        String path = raw.replace('\\', '/');
        while (path.startsWith("./")) {
            path = path.substring(2);
        }
        if (path.isEmpty() || path.startsWith("/") || path.matches("^[A-Za-z]:.*")) {
            throw new IOException("Absolute ZIP path is not allowed");
        }
        String[] originalParts = path.split("/");
        for (String part : originalParts) {
            if (part.isEmpty() || part.equals("..")) {
                throw new IOException("Unsafe ZIP path: " + raw);
            }
        }
        for (String prefix : new String[]{
                "touchHLE_sandbox/" + PACKAGE_ID + "/Documents/",
                PACKAGE_ID + "/Documents/",
                "Documents/",
                "save/Documents/"
        }) {
            if (path.regionMatches(true, 0, prefix, 0, prefix.length())) {
                path = path.substring(prefix.length());
                break;
            }
        }
        if (path.isEmpty()) {
            return null;
        }
        if (!path.toLowerCase(Locale.US).endsWith(".sb")) {
            return null;
        }
        if (path.indexOf('/') < 0) {
            path = "var/" + path;
        }
        return path;
    }

    private static boolean hasFiles(File directory) {
        File[] children = directory.listFiles();
        if (children == null) {
            return false;
        }
        for (File child : children) {
            if (child.isFile() || hasFiles(child)) {
                return true;
            }
        }
        return false;
    }

    private static void zipTree(File root, File output) throws IOException {
        try (ZipOutputStream zip = new ZipOutputStream(new FileOutputStream(output))) {
            addZipEntries(zip, root, "");
        }
    }

    private static void addZipEntries(ZipOutputStream zip, File file, String parent)
            throws IOException {
        File[] children = file.listFiles();
        if (children == null) {
            return;
        }
        for (File child : children) {
            String relative = parent.isEmpty() ? child.getName() : parent + "/" + child.getName();
            if (child.isDirectory()) {
                addZipEntries(zip, child, relative);
            } else if (child.isFile()) {
                zip.putNextEntry(new ZipEntry(relative));
                try (InputStream input = new FileInputStream(child)) {
                    byte[] buffer = new byte[32 * 1024];
                    int read;
                    while ((read = input.read(buffer)) != -1) {
                        zip.write(buffer, 0, read);
                    }
                }
                zip.closeEntry();
            }
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
