// Lunaris from C#: the engine runs inside this process, called through its C interface (crates/lunaris-ffi/include/lunaris.h).
// Every tool takes and returns JSON; errors come back with the engine's stable code and a hint.
using System.Runtime.InteropServices;
using System.Text.Json;

static class Native
{
    const string Lib = "lunaris_ffi";

    [DllImport(Lib)] public static extern IntPtr lunaris_engine_new(ulong seed, [MarshalAs(UnmanagedType.LPUTF8Str)] string root);
    [DllImport(Lib)] public static extern void lunaris_engine_free(IntPtr engine);
    [DllImport(Lib)] public static extern IntPtr lunaris_call(IntPtr engine, [MarshalAs(UnmanagedType.LPUTF8Str)] string tool, [MarshalAs(UnmanagedType.LPUTF8Str)] string args);
    [DllImport(Lib)] public static extern void lunaris_string_free(IntPtr s);
}

sealed class LunarisException : Exception
{
    public string Code { get; }
    public string Hint { get; }
    public LunarisException(string message, string code, string hint) : base(message) { Code = code; Hint = hint; }
}

sealed class Engine : IDisposable
{
    IntPtr handle;

    public Engine(string root, ulong seed = 1) => handle = Native.lunaris_engine_new(seed, root);

    /// <summary>Run one tool; returns its result or throws with the engine's code and hint.</summary>
    public JsonElement Call(string tool, string args = "{}")
    {
        IntPtr p = Native.lunaris_call(handle, tool, args);
        string text = Marshal.PtrToStringUTF8(p)!;
        Native.lunaris_string_free(p);
        JsonElement reply = JsonDocument.Parse(text).RootElement.Clone();
        if (reply.TryGetProperty("error", out JsonElement err))
        {
            JsonElement data = err.TryGetProperty("data", out var d) ? d : default;
            string Field(string name) => data.ValueKind == JsonValueKind.Object && data.TryGetProperty(name, out var v) ? v.GetString() ?? "" : "";
            throw new LunarisException(err.GetProperty("message").GetString() ?? "error", Field("code"), Field("hint"));
        }
        return reply.GetProperty("result");
    }

    public void Dispose()
    {
        if (handle != IntPtr.Zero) { Native.lunaris_engine_free(handle); handle = IntPtr.Zero; }
    }
}

static class Program
{
    static int Main(string[] argv)
    {
        string root = argv.Length > 0 ? argv[0] : Directory.GetCurrentDirectory();
        using var engine = new Engine(root, seed: 5);

        // build a tiny world: a floor, a ball that falls on it, a camera
        engine.Call("spawn", """{"components":{"Name":{"value":"Floor"},"Transform":{"pos":[0,-0.5,0],"scale":[20,1,20]},"Collider":{"shape":1,"size":[1,1,1]},"MeshRenderer":{"mesh":"cube","color":[0.4,0.5,0.4,1]}}}""");
        long ball = engine.Call("spawn", """{"components":{"Name":{"value":"Ball"},"Transform":{"pos":[0,4,0]},"Velocity":{},"Collider":{"shape":0},"RigidBody":{},"MeshRenderer":{"mesh":"sphere","color":[0.9,0.3,0.2,1]}}}""").GetProperty("uid").GetInt64();
        engine.Call("spawn", """{"components":{"Name":{"value":"Camera"},"Transform":{"pos":[0,2,6],"rot":[-0.1,0,0,0.995]},"Camera":{}}}""");

        var step = engine.Call("step", """{"ticks":180}""");
        double y = engine.Call("get", $$"""{"uid":{{ball}},"components":["Transform"]}""").GetProperty("components").GetProperty("Transform").GetProperty("pos")[1].GetDouble();
        Console.WriteLine($"after {step.GetProperty("tick").GetInt64()} ticks the ball rests at y = {y:F3}; state hash {step.GetProperty("hash").GetString()}");

        // errors are ordinary values with a code and a hint
        try { engine.Call("spawn", """{"components":{"Transfrom":{}}}"""); }
        catch (LunarisException e) { Console.WriteLine($"refused: {e.Code} - {e.Hint}"); }

        // a picture, made by the engine's own renderer
        var frame = engine.Call("render", """{"width":320,"height":180}""");
        byte[] png = Convert.FromBase64String(frame.GetProperty("image").GetProperty("data").GetString()!);
        File.WriteAllBytes(Path.Combine(root, "target", "csharp-demo.png"), png);
        Console.WriteLine($"wrote target/csharp-demo.png ({png.Length} bytes, picture hash {frame.GetProperty("hash").GetString()})");
        return Math.Abs(y - 0.5) < 0.05 ? 0 : 1;
    }
}
