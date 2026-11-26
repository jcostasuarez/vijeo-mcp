// Decompila funciones por nombre de símbolo (exportado o demangled), coincidencia por subcadena.
// Uso (headless): -postScript DecompileByName.java <out.c> <subcadena> [<subcadena>...]
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

public class DecompileByName extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 2) {
			printerr("uso: DecompileByName <out.c> <subcadena> [<subcadena>...]");
			return;
		}
		DecompInterface ifc = new DecompInterface();
		ifc.openProgram(currentProgram);
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			for (int i = 1; i < args.length; i++) {
				String wanted = args[i];
				int n = 0;
				FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
				while (it.hasNext() && !monitor.isCancelled()) {
					Function f = it.next();
					String full = f.getName(true);
					if (!full.contains(wanted)) {
						continue;
					}
					n++;
					DecompileResults r = ifc.decompileFunction(f, 180, monitor);
					out.println("// ===== " + full + " @ " + f.getEntryPoint() + "  [" + wanted + "]");
					out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC()
							: "// fallo al decompilar: " + r.getErrorMessage());
				}
				out.println("// ##### \"" + wanted + "\": " + n + " funciones");
			}
		}
		finally {
			ifc.dispose();
		}
	}
}
