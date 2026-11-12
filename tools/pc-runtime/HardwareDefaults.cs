using System;
using System.Collections.Generic;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed class DisplayHardware
    {
        internal int Width, Height;
        internal string GpuName = "未识别专用显存";
        internal string CpuName = "未识别CPU";
        internal ulong DedicatedBytes;
        internal double DedicatedGiB { get { return DedicatedBytes / 1073741824.0; } }
        internal string ResolutionLabel { get { return Width + " × " + Height; } }

        // Full-mip packs need additional room for render targets, the original
        // game and driver allocations. These are conservative defaults, not a
        // measurement of the currently free VRAM or a frame-time guarantee.
        internal static string RecommendedScale(ulong dedicatedBytes)
        {
            double gib = dedicatedBytes / 1073741824.0;
            return gib >= 16 ? "8" : gib >= 6 ? "4" : gib >= 2 ? "2" : null;
        }

        [StructLayout(LayoutKind.Explicit, Size = 220, CharSet = CharSet.Unicode)]
        private struct DeviceMode
        {
            [FieldOffset(68)] internal ushort Size;
            [FieldOffset(172)] internal uint Width;
            [FieldOffset(176)] internal uint Height;
        }
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct AdapterDescription
        {
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] internal string Description;
            internal uint VendorId, DeviceId, SubsystemId, Revision;
            internal UIntPtr DedicatedVideoMemory, DedicatedSystemMemory, SharedSystemMemory;
            internal uint LuidLow;
            internal int LuidHigh;
            internal uint Flags;
        }
        [DllImport("user32.dll", EntryPoint = "EnumDisplaySettingsW", CharSet = CharSet.Unicode)]
        private static extern bool EnumDisplaySettings(string device, int mode, ref DeviceMode settings);
        [DllImport("dxgi.dll")]
        private static extern int CreateDXGIFactory1(ref Guid iid, out IntPtr factory);
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate int EnumAdapters(IntPtr factory, uint index, out IntPtr adapter);
        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate int GetDescription(IntPtr adapter, out AdapterDescription description);

        private static Delegate Method(IntPtr instance, int slot, Type type)
        { return Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(Marshal.ReadIntPtr(instance), slot * IntPtr.Size), type); }

        internal void RefreshDisplayResolution()
        {
            var mode = new DeviceMode { Size = (ushort)Marshal.SizeOf(typeof(DeviceMode)) };
            // Read physical pixels again on every launch; never use DPI-scaled Form coordinates.
            if (EnumDisplaySettings(null, -1, ref mode) && mode.Width > 0 && mode.Height > 0)
            { Width = (int)mode.Width; Height = (int)mode.Height; }
            else { Width = 0; Height = 0; }
        }

        internal static DisplayHardware Detect()
        {
            var result = new DisplayHardware();
            try { result.CpuName = Convert.ToString(Microsoft.Win32.Registry.GetValue(
                @"HKEY_LOCAL_MACHINE\HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString", "未识别CPU")).Trim(); }
            catch (Exception) { }

            result.RefreshDisplayResolution();
            IntPtr factory = IntPtr.Zero;
            try
            {
                Guid iid = new Guid("770aae78-f26f-4dba-a829-253c83d1b387");
                if (CreateDXGIFactory1(ref iid, out factory) < 0 || factory == IntPtr.Zero) return result;
                // Slots follow the Windows SDK dxgi.h IDXGIFactory1Vtbl and
                // IDXGIAdapter1Vtbl, including IUnknown and IDXGIObject.
                var enumerate = (EnumAdapters)Method(factory, 12, typeof(EnumAdapters));
                for (uint i = 0; i < 64; i++)
                {
                    IntPtr adapter;
                    int hr = enumerate(factory, i, out adapter);
                    if (hr < 0) break;
                    if (adapter == IntPtr.Zero) continue;
                    try
                    {
                        AdapterDescription description;
                        var describe = (GetDescription)Method(adapter, 10, typeof(GetDescription));
                        if (describe(adapter, out description) < 0 || (description.Flags & 3) != 0) continue;
                        ulong bytes = description.DedicatedVideoMemory.ToUInt64();
                        if (bytes > result.DedicatedBytes)
                        { result.DedicatedBytes = bytes; result.GpuName = description.Description.Trim(); }
                    }
                    finally { Marshal.Release(adapter); }
                }
            }
            catch (DllNotFoundException) { }
            catch (EntryPointNotFoundException) { }
            catch (COMException) { }
            finally { if (factory != IntPtr.Zero) Marshal.Release(factory); }
            return result;
        }
    }

    internal sealed partial class LauncherForm
    {
        private readonly DisplayHardware detectedHardware;
        private const string AutomaticResolution = "自动（主屏分辨率）";

        private void EnsureResolutionOption(string label)
        { if (displayResolution.Items.IndexOf(label) < 0) displayResolution.Items.Add(label); }

        private string SelectedOutputSize()
        {
            string selected = Convert.ToString(displayResolution.SelectedItem);
            if (selected == "跟随内部") return null;
            if (selected == AutomaticResolution)
                return detectedHardware.Width > 0 ? detectedHardware.Width + "x" + detectedHardware.Height : null;
            string[] parts = selected.Split('×');
            int width, height;
            if (parts.Length == 2 && Int32.TryParse(parts[0].Trim(), out width) && Int32.TryParse(parts[1].Trim(), out height) && width > 0 && height > 0)
                return width + "x" + height;
            throw new InvalidOperationException("输出分辨率格式无效。");
        }

        private void ApplyDetectedGraphicsDefaults()
        {
            // First configuration/reset only: choose from physical display,
            // persist resolved values, and leave later launches to saved settings.
            if(detectedHardware.Width>0 && detectedHardware.Height>0) {
                string output=detectedHardware.ResolutionLabel;EnsureResolutionOption(output);SetCombo(displayResolution,output);
                SelectRenderScale(PcRenderResolution.ScaleForHeight(detectedHardware.Height));
            } else {EnsureResolutionOption(AutomaticResolution);SetCombo(displayResolution,AutomaticResolution);SetCombo(scaleHack,AutomaticRenderScale);}
            windowMode.SelectedIndex=2;fpsCap.SelectedIndex=3;
            fsrMode.SelectedIndex=0;msaaSamples.SelectedIndex=2;anisotropy.SelectedIndex=1;
            sharpen.Checked=false;fastPresent.Checked=true;safeBlackFloor.Checked=false;safeBlackGray.Value=10;gpuPresentCopy.Checked=true;gpuTextOverlay.Checked=true;
            texture2x.Checked=false;texture4x.Checked=true;texture6x.Checked=false;hdTextures.Checked=true;
            UpdateTextureDescription();
            status.Text="已识别："+detectedHardware.GpuName+" · 已按屏幕预置；保存后下次直接读取配置。";
        }
    }
}
