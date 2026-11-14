package org.touchhle.android;

import android.content.Context;
import android.util.Log;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import org.json.JSONArray;
import org.json.JSONObject;

/** Static overlay IDs plus the selected SBIN language bank and English fallback. */
final class OverlayTexts {
    static String[] ids, text, english;
    private static String selectedLanguage = "zh-tieba";

    static void load(Context context) throws Exception { load(context, selectedLanguage); }

    static synchronized void load(Context context, String language) throws Exception {
        if (ids != null) { return; }
        selectedLanguage = language == null ? "zh-tieba" : language.toLowerCase(java.util.Locale.ROOT);
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        try (InputStream in = context.getAssets().open("overlay/subtitles-original.json")) {
            byte[] buffer = new byte[16384]; int n;
            while ((n = in.read(buffer)) != -1) { bytes.write(buffer, 0, n); }
        }
        JSONArray entries = new JSONObject(new String(bytes.toByteArray(), StandardCharsets.UTF_8))
                .getJSONArray("entries");
        ids = new String[entries.length() + 1];
        text = new String[ids.length];
        english = new String[ids.length];
        for (int i = 0; i < entries.length(); i++) {
            JSONObject entry = entries.getJSONObject(i);
            ids[i + 1] = entry.getString("id");
            text[i + 1] = entry.getString("text");
            english[i + 1] = entry.optString("english", "");
        }

        File base = context.getExternalFilesDir(null);
        if (base == null) { base = context.getFilesDir(); }
        File primaryFile = new File(base, "overlay-language/" + selectedLanguage + ".sbin");
        File fallbackFile = new File(base, "overlay-language/en-original.sbin");
        Map<String, String> primary = new HashMap<>();
        Map<String, String> fallback = new HashMap<>();
        // SDL may finish getArguments() on the native startup boundary just
        // after the Java view is constructed. Give the private extracted banks
        // a short bounded window to appear before falling back to JSON text.
        for (int attempt = 0; attempt < 120; attempt++) {
            primary = readBank(primaryFile);
            fallback = readBank(fallbackFile);
            if (!primary.isEmpty() || !fallback.isEmpty() || attempt == 19) break;
            try { Thread.sleep(250); } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt(); break;
            }
        }
        Log.i("touchHLE", "[DStext-bank] language=" + selectedLanguage
                + " primary_entries=" + primary.size() + " english_entries=" + fallback.size()
                + " primary_file=" + primaryFile + " primary_bytes=" + primaryFile.length());
        for (int i = 1; i < ids.length; i++) {
            String id = ids[i];
            String localized = primary.get(id);
            if (!placeholder(localized)) {
                text[i] = localized;
            } else if (!placeholder(fallback.get(id))) {
                text[i] = fallback.get(id);
            }
            String original = fallback.get(id);
            if (!placeholder(original)) { english[i] = original; }
        }
    }

    static String language() { return selectedLanguage; }
    static String value(String id, boolean en) {
        if (ids == null) { return id; }
        for (int i = 1; i < ids.length; i++) {
            if (id.equals(ids[i])) { return en ? english[i] : text[i]; }
        }
        return id;
    }

    private static boolean placeholder(String value) {
        if (value == null) { return true; }
        String trimmed = value.trim();
        return trimmed.isEmpty() || trimmed.equalsIgnoreCase("YES")
                || trimmed.equalsIgnoreCase("NO") || trimmed.contains("XXXXXX");
    }

    private static Map<String, String> readBank(File file) {
        Map<String, String> result = new HashMap<>();
        if (!file.isFile() || file.length() <= 0 || file.length() > 2L * 1024 * 1024) return result;
        try (InputStream input = new FileInputStream(file)) {
            ByteArrayOutputStream raw = new ByteArrayOutputStream((int) file.length());
            byte[] buffer = new byte[16384];
            int count;
            while ((count = input.read(buffer)) != -1) raw.write(buffer, 0, count);
            byte[] bytes = raw.toByteArray();
            if (bytes.length < 8 || bytes[0] != 'S' || bytes[1] != 'B' || bytes[2] != 'I'
                    || bytes[3] != 'N' || bytes[4] != 2) return result;
            byte[] stringsChunk = null, dataChunk = null, ohdrChunk = null;
            int cursor = 8;
            while (cursor + 12 <= bytes.length) {
                String tag = new String(bytes, cursor, 4, StandardCharsets.US_ASCII);
                int length = le32(bytes, cursor + 4);
                int payload = cursor + 12, end = payload + length;
                int aligned = (end + 3) & ~3;
                if (length < 0 || end < payload || aligned > bytes.length) return result;
                // The native reader validates the FNV checksum. Android only
                // needs a bounded read of the same immutable bank; accepting
                // a checksum mismatch here keeps vendor-repacked IPA banks
                // usable while the structural bounds below remain strict.
                if ("STRS".equals(tag)) stringsChunk = slice(bytes, payload, end);
                else if ("DATA".equals(tag)) dataChunk = slice(bytes, payload, end);
                else if ("OHDR".equals(tag)) ohdrChunk = slice(bytes, payload, end);
                cursor = aligned;
            }
            if (stringsChunk == null || dataChunk == null || ohdrChunk == null
                    || ohdrChunk.length != 12) return result;
            String[] strings = unpackStrings(stringsChunk);
            int list = le32(ohdrChunk, 8) >>> 3;
            if (list < 0 || list + 8 > dataChunk.length || le32(dataChunk, list) != 16) return result;
            int entryCount = le32(dataChunk, list + 4);
            if (entryCount < 0 || list + 8L + entryCount * 8L != dataChunk.length) return result;
            for (int i = 0; i < entryCount; i++) {
                int at = list + 8 + i * 8;
                int name = le16(dataChunk, at), value = le16(dataChunk, at + 2);
                if (name <= 0 || name > strings.length || value > strings.length) continue;
                result.put(strings[name - 1], value == 0 ? "" : strings[value - 1]);
            }
        } catch (Exception ignored) {
            return new HashMap<>();
        }
        return result;
    }

    private static String[] unpackStrings(byte[] bytes) {
        if (bytes.length < 4) return new String[0];
        int count = le32(bytes, 0), cursor = 4;
        if (count < 0 || count > 100000) return new String[0];
        String[] result = new String[count];
        for (int i = 0; i < count; i++) {
            if (cursor + 4 > bytes.length) return new String[0];
            int length = le32(bytes, cursor); cursor += 4;
            if (length < 0 || cursor + length > bytes.length) return new String[0];
            result[i] = new String(bytes, cursor, length, StandardCharsets.UTF_8);
            cursor += length;
        }
        return cursor == bytes.length ? result : new String[0];
    }

    private static byte[] slice(byte[] bytes, int start, int end) {
        return java.util.Arrays.copyOfRange(bytes, start, end);
    }
    private static int le16(byte[] bytes, int at) {
        return (bytes[at] & 0xff) | ((bytes[at + 1] & 0xff) << 8);
    }
    private static int le32(byte[] bytes, int at) {
        return (bytes[at] & 0xff) | ((bytes[at + 1] & 0xff) << 8)
                | ((bytes[at + 2] & 0xff) << 16) | ((bytes[at + 3] & 0xff) << 24);
    }
    private static int fnv1(byte[] bytes, int start, int end) {
        int value = 0x811c9dc5;
        for (int i = start; i < end; i++) value = value * 0x01000193 ^ (bytes[i] & 0xff);
        return value;
    }
}
