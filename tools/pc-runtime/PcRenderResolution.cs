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
        internal static string Size(int scale)
        {
            if (scale <= 0) throw new ArgumentOutOfRangeException("scale");
            return checked(scale * BaseWidth) + " × " + checked(scale * BaseHeight);
        }
    }
}
