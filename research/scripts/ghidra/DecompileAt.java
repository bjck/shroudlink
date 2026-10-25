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
            // "@<addr>[:<len>]": a function entry the auto-analysis missed (.pdata has it, and
            // its length); created in memory, the project stays read-only
            boolean entry = args[i].startsWith("@");
            String addr = entry ? args[i].substring(1) : args[i];
            long length = 0x10;
            if (addr.contains(":")) {
                length = Long.decode(addr.substring(addr.indexOf(':') + 1));
                addr = addr.substring(0, addr.indexOf(':'));
            }
            Function f = entry ? getFunctionAt(toAddr(addr)) : getFunctionContaining(toAddr(addr));
            if (f == null && entry) {
                // the analysis may have merged it into the function before it
                Function merged = getFunctionContaining(toAddr(addr));
                if (merged != null) {
                    removeFunction(merged);
                    createFunction(merged.getEntryPoint(), null);
                }
                // instructions decoded at the wrong alignment or data may be left over
                clearListing(toAddr(addr), toAddr(addr).add(length - 1));
                boolean ok = disassemble(toAddr(addr));
                f = createFunction(toAddr(addr), null);
                if (f == null) {
                    println("could not create a function at " + addr + " (disassembled: " + ok + ", instruction: " + getInstructionAt(toAddr(addr)) + ", data: " + getDataAt(toAddr(addr)) + ")");
                }
            }
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
