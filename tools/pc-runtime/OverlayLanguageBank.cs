using System;
using System.IO;
using System.IO.Compression;

namespace DeadSpaceResources
{
    internal sealed class OverlayLanguageFiles
    {
        internal string Primary;
        internal string English;
    }

    internal static class OverlayLanguageBank
    {
        internal static OverlayLanguageFiles Prepare(string ipaPath, IpaLanguage language, string dataRoot)
        {
            if (language == null) return null;
            string directory = Path.Combine(dataRoot, "overlay-language");
            Directory.CreateDirectory(directory);
            string primary = Path.Combine(directory, language.Id + ".sbin");
            Extract(ipaPath, language.Directory, primary);
            string english = null;
            if (!String.Equals(language.Directory, "ENG_US", StringComparison.OrdinalIgnoreCase))
            {
                english = Path.Combine(directory, "en-original.sbin");
                Extract(ipaPath, "ENG_US", english);
            }
            return new OverlayLanguageFiles { Primary = primary, English = english };
        }

        private static void Extract(string ipaPath, string languageDirectory, string destination)
        {
            using (var input = new FileStream(ipaPath, FileMode.Open, FileAccess.Read, FileShare.Read))
            using (var zip = new ZipArchive(input, ZipArchiveMode.Read, false))
            {
                ZipArchiveEntry plist = null;
                foreach (var entry in zip.Entries)
                    if (entry.FullName.StartsWith("Payload/", StringComparison.Ordinal)
                        && entry.FullName.EndsWith(".app/Info.plist", StringComparison.Ordinal)
                        && entry.FullName.Split('/').Length == 3)
                    {
                        if (plist != null) throw new InvalidDataException("Ambiguous IPA application.");
                        plist = entry;
                    }
                if (plist == null) throw new InvalidDataException("IPA application plist is missing.");
                string prefix = plist.FullName.Substring(0, plist.FullName.Length - "Info.plist".Length);
                string member = prefix + "published/stringdata/" + languageDirectory + "/deadspace_iphone.bin";
                ZipArchiveEntry bank = zip.GetEntry(member);
                if (bank == null || bank.Length <= 0 || bank.Length > 2L * 1024 * 1024)
                    throw new InvalidDataException("Selected IPA language bank is missing or too large.");
                string temporary = destination + ".tmp-" + Guid.NewGuid().ToString("N");
                try
                {
                    using (var source = bank.Open())
                    using (var output = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                        source.CopyTo(output);
                    if (File.Exists(destination)) File.Replace(temporary, destination, null);
                    else File.Move(temporary, destination);
                }
                finally { if (File.Exists(temporary)) File.Delete(temporary); }
            }
        }
    }
}
