/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
package org.touchhle.android;

/** Original HUD_NOTIFICATION_CHAPTER anchors in the game's 480 x 320 space. */
final class ChapterOverlayGeometry {
    final float left, top, right, bottom, textX, numberY, detailsY, textWidth, unitY;

    ChapterOverlayGeometry(float width, float height, boolean stretch) {
        if (width <= 0 || height <= 0) { throw new IllegalArgumentException("Invalid viewport"); }
        float contentWidth = width, contentHeight = height;
        if (!stretch) {
            if (width / height > 1.5f) { contentWidth = height * 1.5f; }
            else { contentHeight = width / 1.5f; }
        }
        float sx = contentWidth / 480f, sy = contentHeight / 320f;
        // (479 * .5010439157, 319 * .5015674233) == (240, 160).
        float ax = (width - contentWidth) / 2f + 240f * sx;
        float ay = (height - contentHeight) / 2f + 160f * sy;
        left = ax - 4f * sx;
        top = ay - 57f * sy;
        right = ax + 226f * sx;
        bottom = ay + 2f * sy;
        textX = ax + 113f * sx;
        numberY = ay - 34.5f * sy;  // TEXT_CHAPTER_NAME: (26,-40)..(200,-29)
        detailsY = ay - 19f * sy;   // TEXT_CHAPTER_DETAILS: (26,-26)..(200,-12)
        textWidth = 174f * sx;
        unitY = sy;
    }
}
