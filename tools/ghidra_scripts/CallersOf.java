// Árbol de llamantes de una o varias funciones (por dirección o por nombre de símbolo, p.ej. importaciones
// como "TransformToPublic"), hasta una profundidad dada. Sirve para subir desde una primitiva hasta el
// comando de UI o método COM que la dispara.
// Uso (headless): -postScript CallersOf.java <out.txt> <profundidad> <addr|símbolo> [...]
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.HashSet;
import java.util.Set;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;

public class CallersOf extends GhidraScript {
	private PrintWriter out;

	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 3) {
			printerr("uso: CallersOf <out.txt> <profundidad> <addr|símbolo> [...]");
			return;
		}
		int depth = Integer.parseInt(args[1]);
		try (PrintWriter w = new PrintWriter(new FileWriter(args[0]))) {
			out = w;
			for (int i = 2; i < args.length; i++) {
				String a = args[i];
				Set<Address> roots = new HashSet<>();
				if (a.matches("[0-9a-fA-F]{6,8}")) {
					roots.add(toAddr(a));
				}
				else {
					SymbolIterator it = currentProgram.getSymbolTable().getAllSymbols(true);
					while (it.hasNext()) {
						Symbol s = it.next();
						if (s.getName().contains(a)) {
							roots.add(s.getAddress());
						}
					}
				}
				for (Address r : roots) {
					Function f = getFunctionContaining(r);
					out.println("# " + a + " @ " + r + (f != null ? " (" + f.getName() + ")" : ""));
					walk(r, 1, depth, new HashSet<>());
				}
			}
		}
	}

	private void walk(Address target, int level, int max, Set<Address> seen) {
		if (level > max) {
			return;
		}
		Set<Function> callers = new HashSet<>();
		for (Reference ref : getReferencesTo(target)) {
			Function f = getFunctionContaining(ref.getFromAddress());
			String label = f != null ? f.getName() + "@" + f.getEntryPoint() : "?";
			if (f == null || callers.add(f)) {
				out.println("  ".repeat(level) + "<- " + label + "  [" + ref.getReferenceType() + " desde " + ref.getFromAddress() + "]");
				if (f != null && seen.add(f.getEntryPoint())) {
					walk(f.getEntryPoint(), level + 1, max, seen);
				}
			}
		}
	}
}
