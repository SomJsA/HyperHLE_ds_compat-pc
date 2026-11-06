package org.touchhle.android;

import android.content.Context;
import android.net.Uri;

import java.io.File;
import java.io.IOException;
import java.io.InputStream;

/** Android storage/filesystem adapter around the pure-Java save importer. */
final class SaveArchiveImporter {
    private SaveArchiveImporter() {}

    static SaveArchiveCore.Result importZip(Context context, Uri uri) throws IOException {
        File base = context.getExternalFilesDir(null);
        if (base == null) {
            base = context.getFilesDir();
        }
        try (InputStream input = context.getContentResolver().openInputStream(uri)) {
            if (input == null) {
                throw new IOException("Could not open ZIP");
            }
            return SaveArchiveCore.importZip(input, base);
        }
    }
}
