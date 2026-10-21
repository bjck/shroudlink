// Lists code/data references to the given addresses (each a single address or
// start-end range) and decompiles every referencing function.
// args: <outDir> <addr|start-end> [...]
//@category enshroudedChests

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashSet;
import java.util.Set;

public class RefsTo extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args[0]);
        outDir.mkdirs();
        Set<Function> functions = new LinkedHashSet<>();
        try (PrintWriter index = new PrintWriter(new File(outDir, "refs.txt"), StandardCharsets.UTF_8)) {
            for (int i = 1; i < args.length; i++) {
                String[] parts = args[i].split("-");
                Address start = toAddr(parts[0]);
                Address end = parts.length > 1 ? toAddr(parts[1]) : start;
                index.println("TARGET " + args[i]);
                for (Address a = start; a.compareTo(end) <= 0; a = a.add(1)) {
                    for (Reference ref : getReferencesTo(a)) {
                        Function f = getFunctionContaining(ref.getFromAddress());
                        index.println("    " + a + " <- " + ref.getFromAddress() + " " + ref.getReferenceType()
                                + (f == null ? " <no function>" : " in " + f.getName() + " @ " + f.getEntryPoint()));
                        if (f != null) {
                            functions.add(f);
                        }
                    }
                }
            }
        }
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        for (Function f : functions) {
            DecompileResults res = decomp.decompileFunction(f, 300, monitor);
            try (PrintWriter w = new PrintWriter(new File(outDir, f.getEntryPoint() + ".c"), StandardCharsets.UTF_8)) {
                w.println("// " + f.getName() + " @ " + f.getEntryPoint());
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
