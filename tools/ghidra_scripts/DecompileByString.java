// Decompila las funciones que referencian cadenas concretas (p.ej. los nombres "CClase::Metodo"
// que Vijeo usa en sus trazas). Independiente de direcciones: sirve igual para otras versiones.
// Uso (headless): -postScript DecompileByString.java <out.c> <cadena> [<cadena>...]
//   Coincidencia exacta del contenido de la cadena (ASCII o UTF-16).
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.LinkedHashSet;
import java.util.Set;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

public class DecompileByString extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 2) {
			printerr("uso: DecompileByString <out.c> <cadena> [<cadena>...]");
			return;
		}
		DecompInterface ifc = new DecompInterface();
		ifc.openProgram(currentProgram);
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			for (int i = 1; i < args.length; i++) {
				String wanted = args[i];
				Set<Function> funcs = new LinkedHashSet<>();
				Set<Address> strAddrs = findStrings(wanted);
				for (Address sa : strAddrs) {
					for (Reference r : getReferencesTo(sa)) {
						Function f = getFunctionContaining(r.getFromAddress());
						if (f != null) {
							funcs.add(f);
						}
					}
				}
				out.println("// ##### \"" + wanted + "\": cadenas en " + strAddrs + ", funciones " + funcs.size());
				for (Function f : funcs) {
					DecompileResults res = ifc.decompileFunction(f, 180, monitor);
					out.println("// ===== " + f.getName() + " @ " + f.getEntryPoint() + "  [" + wanted + "]");
					out.println(res.decompileCompleted() ? res.getDecompiledFunction().getC()
							: "// fallo al decompilar: " + res.getErrorMessage());
				}
			}
		}
		finally {
			ifc.dispose();
		}
	}

	private Set<Address> findStrings(String wanted) {
		Set<Address> found = new LinkedHashSet<>();
		DataIterator it = currentProgram.getListing().getDefinedData(true);
		while (it.hasNext() && !monitor.isCancelled()) {
			Data d = it.next();
			if (!d.hasStringValue()) {
				continue;
			}
			Object v = d.getValue();
			if (v != null && wanted.equals(v.toString())) {
				found.add(d.getAddress());
			}
		}
		return found;
	}
}
