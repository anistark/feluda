# .NET Example Project

This is a simple .NET console application used to test Feluda's .NET dependency analysis.

## Dependencies

**Direct Dependencies:**
- `Newtonsoft.Json` (13.0.3) - MIT License
- `Serilog` (3.1.1) - Apache-2.0 License
- `Microsoft.Extensions.Configuration` (8.0.0) - MIT License
- `Dapper` (2.1.35) - Apache-2.0 License

**Expected Transitive Dependencies:**
- `Microsoft.Extensions.Configuration.Abstractions`
- `Microsoft.Extensions.Primitives`
- And more...

## Testing Feluda

Run Feluda on this project:

```bash
feluda -p examples/dotnet-example
```

With debug output:
```bash
feluda -p examples/dotnet-example -d
```
