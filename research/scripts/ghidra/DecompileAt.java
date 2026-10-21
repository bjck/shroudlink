// Decompiles the functions containing the given addresses.
// args: <outDir> <addr1> [addr2 ...]
//@category enshroudedChests

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;

public class DecompileAt extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args[0]);
        outDir.mkdirs();

        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);

        for (int i = 1; i < args.length; i++) {
            Function f = getFunctionContaining(toAddr(args[i]));
            if (f == null) {
                println("no function at " + args[i]);
                continue;
            }
            DecompileResults res = decomp.decompileFunction(f, 300, monitor);
            try (PrintWriter w = new PrintWriter(new File(outDir, f.getEntryPoint() + ".c"), StandardCharsets.UTF_8)) {
                w.println("// " + f.getName() + " @ " + f.getEntryPoint() + " (requested " + args[i] + ")");
                w.print("// callers:");
                for (Reference ref : getReferencesTo(f.getEntryPoint())) {
                    Function caller = getFunctionContaining(ref.getFromAddress());
                    w.print(" " + ref.getFromAddress() + (caller == null ? "" : "(" + caller.getEntryPoint() + ")"));
                }
                w.println();
                w.println(res.decompileCompleted() ? res.getDecompiledFunction().getC() : "// decompile failed: " + res.getErrorMessage());
            }
        }
        decomp.dispose();
    }
}
