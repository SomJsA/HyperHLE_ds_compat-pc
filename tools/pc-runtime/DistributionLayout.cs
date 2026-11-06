using System;
using System.IO;

namespace DeadSpaceBootstrap
{
    internal static class DistributionLayout
    {
        // New releases keep only the launcher at the top level. Flat legacy
        // packages remain readable; never move files in an installed package.
        internal static string Game(string package)
        {string path=Path.Combine(package,"Game");return Directory.Exists(path)?path:package;}
        internal static string Assets(string package)
        {string path=Path.Combine(package,"Assets");return Directory.Exists(path)?path:package;}
        internal static string PortableData(string package)
        {return Game(package)==package?package:Path.Combine(package,"UserData");}
        internal static string ResolveIpa(string package,string selected)
        {
            string bundled=Path.Combine(Game(package),"DeadSpace.ipa");
            // Only the old package-relative default follows the new layout.
            // Explicit external absolute selections are never substituted.
            if(String.IsNullOrWhiteSpace(selected)||selected=="DeadSpace.ipa")return bundled;
            return Path.GetFullPath(Path.IsPathRooted(selected)?selected:Path.Combine(package,selected));
        }
    }
}
