// Lista las cadenas (ASCII o UTF-16) que casan con una regex y las funciones que las referencian.
// Sirve para localizar funcionalidades por sus mensajes o nombres de fichero sin conocer direcciones.
// Uso (headless): -postScript StringsMatching.java <out.txt> <regex> [<regex>...]
//   La regex se aplica con find() y sin distinguir mayúsculas.
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.LinkedHashSet;
import java.util.Set;
import java.util.regex.Pattern;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

public class StringsMatching extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 2) {
			printerr("uso: StringsMatching <out.txt> <regex> [<regex>...]");
			return;
		}
		Pattern[] pats = new Pattern[args.length - 1];
		for (int i = 1; i < args.length; i++) {
			pats[i - 1] = Pattern.compile(args[i], Pattern.CASE_INSENSITIVE);
		}
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			DataIterator it = currentProgram.getListing().getDefinedData(true);
			while (it.hasNext() && !monitor.isCancelled()) {
				Data d = it.next();
				if (!d.hasStringValue()) {
					continue;
				}
				Object v = d.getValue();
				if (v == null) {
					continue;
				}
				String s = v.toString();
				boolean hit = false;
				for (Pattern p : pats) {
					if (p.matcher(s).find()) {
						hit = true;
						break;
					}
				}
				if (!hit) {
					continue;
				}
				Set<String> funcs = new LinkedHashSet<>();
				for (Reference r : getReferencesTo(d.getAddress())) {
					Function f = getFunctionContaining(r.getFromAddress());
					funcs.add(f != null ? f.getName() + "@" + f.getEntryPoint() : "?@" + r.getFromAddress());
				}
				out.println(d.getAddress() + "\t" + s.replace("\n", "\\n").replace("\r", "\\r") + "\t" + funcs);
			}
		}
	}
}
