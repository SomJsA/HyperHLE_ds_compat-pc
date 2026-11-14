using System;

namespace DeadSpaceBootstrap
{
    // Dead Space's landscape game surface is 480 x 320 before scale-hack.
    // Physical output pixels, never Windows DPI logical coordinates, set the
    // automatic reference. Keep this calculation independent of UI strings.
    internal static class PcRenderResolution
    {
        internal const int BaseWidth = 480, BaseHeight = 320;
        internal static int ScaleForHeight(int height)
        {
            if (height <= 0) throw new ArgumentOutOfRangeException("height");
            // User policy:1080p=>4x,1440p=>6x,2160p=>8x. Round up to an even game scale.
            int scale = checked((int)((((long)height + BaseHeight * 2 - 1) / (BaseHeight * 2)) * 2));
            checked { int width = scale * BaseWidth; int actualHeight = scale * BaseHeight;
                if (width < 1 || actualHeight < height) throw new ArgumentOutOfRangeException("height"); }
            return scale;
        }

        // Legacy IPAs expose a different guest surface (the phone builds are
        // 320x480 and the iPad build is 768x1024).  Their automatic mode must
        // follow that guest height, rather than applying the PC 480x320 policy
        // and accidentally turning a saved/selected legacy install into a
        // fixed 2x or an oversized 6x render target.
        internal static int ScaleForGuestHeight(int height, int guestHeight)
        {
            if (height <= 0) throw new ArgumentOutOfRangeException("height");
            if (guestHeight <= 0) throw new ArgumentOutOfRangeException("guestHeight");
            long scale = ((long)height + guestHeight - 1) / guestHeight;
            if (scale < 1 || scale > Int32.MaxValue) throw new ArgumentOutOfRangeException("height");
            return (int)scale;
        }
        internal static string Size(int scale)
        {
            if (scale <= 0) throw new ArgumentOutOfRangeException("scale");
            return checked(scale * BaseWidth) + " × " + checked(scale * BaseHeight);
        }
    }
}
