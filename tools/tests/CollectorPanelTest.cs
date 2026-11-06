using System;
using System.Drawing;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using System.Windows.Forms;
class CollectorPanelTest
{
    static void Check(bool condition,string message){if(!condition)throw new Exception(message);}
    [STAThread] static void Main(string[] args)
    {
        var assembly=Assembly.LoadFrom(Path.GetFullPath(args[0]));var type=assembly.GetType("DeadSpaceBootstrap.LauncherForm",true);
        var owner=FormatterServices.GetUninitializedObject(type);var flags=BindingFlags.NonPublic|BindingFlags.Instance;
        foreach(var pair in new object[][]{new object[]{"playDiagnostics",new CheckBox{Checked=true}},new object[]{"diagnosticDetail",new ComboBox{DropDownStyle=ComboBoxStyle.DropDownList}},new object[]{"diagnosticStatus",new Label()},new object[]{"problemNote",new TextBox()}})type.GetField((string)pair[0],flags).SetValue(owner,pair[1]);
        using(var outer=new Panel{Size=new Size(710,1000),AutoScroll=true})
        {
            var old=new Label{Top=184,Left=30,Width=600,Text="existing setting"};outer.Controls.Add(old);
            var advanced=new Panel{Top=454,Height=500,Tag="advanced-content",Visible=false};outer.Controls.Add(advanced);
            type.GetMethod("BuildPlayDiagnostics",flags).Invoke(owner,new object[]{outer});
            var combo=(ComboBox)type.GetField("diagnosticDetail",flags).GetValue(owner);
            Check(combo.Items.Count==3&&combo.SelectedIndex==2,"Collector build must default to super, three modes");
            Check(combo.Width>=400&&combo.Height>=20,"Diagnostic combo collapsed");
            Check(combo.Parent!=outer&&combo.Parent.Parent==outer,"Diagnostics need their own panel, outside advanced row remapping");
            Check(old.Parent==outer&&old.Top==554,"Existing rows moved into diagnostic panel");
            Check(advanced.Parent==outer&&advanced.Top==824,"Advanced panel moved inconsistently");
            foreach(Control control in combo.Parent.Controls)Check(control.Top>=0&&control.Bottom<=combo.Parent.Height,"Diagnostic control outside panel: "+control.GetType().Name);
            Check(outer.AutoScrollMinSize.Height>=old.Bottom+16,"Invisible parent caused scroll extent to collapse");
            string picture=Path.GetFullPath(args[1]);Check(!File.Exists(picture),"Image output must be new");
            combo.Parent.CreateControl();using(var image=new Bitmap(710,360)){combo.Parent.DrawToBitmap(image,new Rectangle(0,0,710,360));image.Save(picture);}
            Console.WriteLine("PASS super default / three nonzero-width modes / separate diagnostic panel / prior settings+advanced positions / all child bounds / hidden-form scroll extent / rendered preview");
        }
    }
}
