// Read-only Configuration Manager API; avoid slow WMI/PnpDevice provider startup.
// Property keys verified against Windows SDK 10.0.26100 shared/devpkey.h.
export const miniTopologyScript = String.raw`
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class MiniTopology {
  [StructLayout(LayoutKind.Sequential)] public struct Key { public Guid fmtid; public uint pid; }
  public class Node { public string id, parent, container, name; public string[] location; public uint address; }
  [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode)] static extern uint CM_Get_Device_ID_List_SizeW(out uint size, string filter, uint flags);
  [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode)] static extern uint CM_Get_Device_ID_ListW(string filter, [Out] char[] data, uint size, uint flags);
  [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode)] static extern uint CM_Locate_DevNodeW(out uint node, string id, uint flags);
  [DllImport("cfgmgr32.dll")] static extern uint CM_Get_Parent(out uint parent, uint node, uint flags);
  [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode)] static extern uint CM_Get_Device_IDW(uint node, StringBuilder id, uint size, uint flags);
  [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode)] static extern uint CM_Get_DevNode_PropertyW(uint node, ref Key key, out uint type, byte[] data, ref uint size, uint flags);
  static byte[] Property(uint node, string guid, uint pid) {
    Key key = new Key { fmtid = new Guid(guid), pid = pid }; uint type, size = 0;
    CM_Get_DevNode_PropertyW(node, ref key, out type, null, ref size, 0);
    if (size == 0 || size > 65536) return new byte[0];
    byte[] data = new byte[size];
    return CM_Get_DevNode_PropertyW(node, ref key, out type, data, ref size, 0) == 0 ? data : new byte[0];
  }
  public static Node[] Read() {
    const string deviceKey = "a45c254e-df1c-4efd-8020-67d146a850e0";
    uint size; const uint flags = 0x101; // USB enumerator + present devices only.
    if (CM_Get_Device_ID_List_SizeW(out size, "USB", flags) != 0 || size > 1048576) throw new Exception("USB enumeration failed");
    char[] ids = new char[size];
    if (CM_Get_Device_ID_ListW("USB", ids, size, flags) != 0) throw new Exception("USB enumeration changed; rescan");
    var result = new List<Node>();
    foreach (string id in new string(ids).Split(new char[] {'\0'}, StringSplitOptions.RemoveEmptyEntries)) {
      if (!System.Text.RegularExpressions.Regex.IsMatch(id, @"^USB\\VID_(CAFE&PID_400[124]|(345F|534D)&PID_2130|05E3&PID_06(10|26))", System.Text.RegularExpressions.RegexOptions.IgnoreCase) || id.Contains("&MI_")) continue;
      uint node, parent;
      if (CM_Locate_DevNodeW(out node, id, 0) != 0) continue;
      var parentId = new StringBuilder(1024);
      if (CM_Get_Parent(out parent, node, 0) == 0) CM_Get_Device_IDW(parent, parentId, 1024, 0);
      byte[] container = Property(node, "8c7ed206-3f8a-4827-b3ab-ae9e1faefc6c", 2);
      byte[] address = Property(node, deviceKey, 30);
      string[] location = Encoding.Unicode.GetString(Property(node, deviceKey, 37)).Split(new char[] {'\0'}, StringSplitOptions.RemoveEmptyEntries);
      string name = Encoding.Unicode.GetString(Property(node, deviceKey, 14)).TrimEnd('\0');
      result.Add(new Node { id = id, parent = parentId.ToString(), container = container.Length == 16 ? new Guid(container).ToString() : "",
        location = location, address = address.Length == 4 ? BitConverter.ToUInt32(address, 0) : 0, name = name });
    }
    return result.ToArray();
  }
}
'@ -ErrorAction Stop
@([MiniTopology]::Read()) | ConvertTo-Json -Depth 4 -Compress`
