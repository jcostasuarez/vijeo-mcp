// Decompila las funciones que contienen las direcciones indicadas y las vuelca a un fichero C.
// Uso (headless): analyzeHeadless <proj_dir> <proj> -process <bin> -noanalysis
//                   -scriptPath <tools/ghidra_scripts> -postScript DecompileAt.java <out.c> <addr> [<addr>...]
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class DecompileAt extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 2) {
			printerr("uso: DecompileAt <out.c> <addr> [<addr>...]");
			return;
		}
		DecompInterface ifc = new DecompInterface();
		ifc.openProgram(currentProgram);
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			for (int i = 1; i < args.length; i++) {
				Address a = toAddr(args[i]);
				Function f = getFunctionContaining(a);
				if (f == null) {
					f = createFunction(a, null);
				}
				if (f == null) {
					out.println("// no hay funcion en " + args[i]);
					continue;
				}
				DecompileResults r = ifc.decompileFunction(f, 180, monitor);
				out.println("// ===== " + f.getName() + " @ " + f.getEntryPoint() + " (pedido " + args[i] + ")");
				out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC()
						: "// fallo al decompilar: " + r.getErrorMessage());
			}
		}
		finally {
			ifc.dispose();
		}
	}
}
