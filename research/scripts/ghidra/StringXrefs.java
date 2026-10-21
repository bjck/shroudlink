// Finds defined/raw strings, lists referencing functions and decompiles them.
// args: <outDir> <string1> [string2 ...]
//@category enshroudedChests

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.Reference;

import java.io.File;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.util.LinkedHashSet;
import java.util.Set;

public class StringXrefs extends GhidraScript {

    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        File outDir = new File(args[0]);
        outDir.mkdirs();

        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        Memory mem = currentProgram.getMemory();
        Set<Function> done = new LinkedHashSet<>();

        try (PrintWriter index = new PrintWriter(new File(outDir, "index.txt"), StandardCharsets.UTF_8)) {
            for (int i = 1; i < args.length; i++) {
                byte[] needle = (args[i] + "\0").getBytes(StandardCharsets.US_ASCII);
                Address addr = mem.getMinAddress();
                while ((addr = mem.findBytes(addr, needle, null, true, monitor)) != null) {
                    index.printf("STRING %s @ %s%n", args[i], addr);
                    for (Reference ref : getReferencesTo(addr)) {
                        Function f = getFunctionContaining(ref.getFromAddress());
                        String name = f == null ? "<no function>" : f.getName() + " @ " + f.getEntryPoint();
                        index.printf("    xref from %s in %s%n", ref.getFromAddress(), name);
                        if (f != null && done.add(f)) {
                            DecompileResults res = decomp.decompileFunction(f, 120, monitor);
                            try (PrintWriter w = new PrintWriter(new File(outDir, f.getEntryPoint() + ".c"), StandardCharsets.UTF_8)) {
                                w.println("// " + f.getName() + " @ " + f.getEntryPoint());
                                w.println(res.decompileCompleted() ? res.getDecompiledFunction().getC() : "// decompile failed: " + res.getErrorMessage());
                            }
                        }
                    }
                    addr = addr.add(1);
                }
            }
        }
        decomp.dispose();
    }
}
