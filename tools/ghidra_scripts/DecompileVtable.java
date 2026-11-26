// Decompila los métodos de la vtable (RTTI) de una o varias clases.
// Uso (headless): -postScript DecompileVtable.java <out.c> <Clase> [<Clase>...]
//   Busca el símbolo "<Clase>::vftable" creado por el analizador RTTI de Ghidra.
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Namespace;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;

public class DecompileVtable extends GhidraScript {
	private static final int MAX_SLOTS = 64;

	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 2) {
			printerr("uso: DecompileVtable <out.c> <Clase> [<Clase>...]");
			return;
		}
		DecompInterface ifc = new DecompInterface();
		ifc.openProgram(currentProgram);
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			for (int i = 1; i < args.length; i++) {
				String cls = args[i];
				boolean found = false;
				SymbolIterator it = currentProgram.getSymbolTable().getSymbols("vftable");
				while (it.hasNext()) {
					Symbol s = it.next();
					Namespace ns = s.getParentNamespace();
					if (ns == null || !cls.equals(ns.getName())) {
						continue;
					}
					found = true;
					out.println("// ##### " + cls + "::vftable @ " + s.getAddress());
					dumpVtable(out, ifc, s.getAddress(), cls);
				}
				if (!found) {
					out.println("// ##### " + cls + "::vftable no encontrada");
				}
			}
		}
		finally {
			ifc.dispose();
		}
	}

	private void dumpVtable(PrintWriter out, DecompInterface ifc, Address vt, String cls) throws Exception {
		for (int slot = 0; slot < MAX_SLOTS; slot++) {
			Address entry = vt.add(slot * 4L);
			int raw = getInt(entry);
			Address target = toAddr(Integer.toUnsignedLong(raw));
			Function f = getFunctionAt(target);
			if (f == null) {
				f = getFunctionContaining(target);
			}
			if (f == null) {
				out.println("// fin de vtable en slot " + slot);
				return;
			}
			DecompileResults r = ifc.decompileFunction(f, 180, monitor);
			out.println("// ===== " + cls + " slot " + slot + " (+0x" + Integer.toHexString(slot * 4) + "): "
					+ f.getName() + " @ " + f.getEntryPoint());
			out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC()
					: "// fallo al decompilar: " + r.getErrorMessage());
		}
	}
}
