using System;
using Newtonsoft.Json;
using Serilog;

namespace DotNetExample
{
    class Program
    {
        static void Main(string[] args)
        {
            Log.Logger = new LoggerConfiguration()
                .WriteTo.Console()
                .CreateLogger();

            var data = new { Message = "Hello from .NET!" };
            var json = JsonConvert.SerializeObject(data);

            Log.Information("Serialized: {Json}", json);
            Log.CloseAndFlush();
        }
    }
}
