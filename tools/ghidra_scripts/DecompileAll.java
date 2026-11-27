// Decompila todas las funciones del programa (o las de un rango de direcciones) y las vuelca a un fichero C.
// Uso (headless): -postScript DecompileAll.java <out.c> [<desde> <hasta>]
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

public class DecompileAll extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 1) {
			printerr("uso: DecompileAll <out.c> [<desde> <hasta>]");
			return;
		}
		Address from = args.length >= 3 ? toAddr(args[1]) : null;
		Address to = args.length >= 3 ? toAddr(args[2]) : null;
		DecompInterface ifc = new DecompInterface();
		ifc.openProgram(currentProgram);
		int n = 0;
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
			while (it.hasNext() && !monitor.isCancelled()) {
				Function f = it.next();
				Address a = f.getEntryPoint();
				if (from != null && (a.compareTo(from) < 0 || a.compareTo(to) > 0)) {
					continue;
				}
				if (f.isThunk() || f.isExternal()) {
					continue;
				}
				DecompileResults r = ifc.decompileFunction(f, 120, monitor);
				out.println("// ===== " + f.getName() + " @ " + a);
				out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC()
						: "// fallo al decompilar: " + r.getErrorMessage());
				n++;
			}
		}
		println("funciones: " + n);
	}
}
