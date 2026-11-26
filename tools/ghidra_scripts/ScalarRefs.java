// Funciones que usan un valor inmediato (ids de recursos de cadena, de comandos, etc.) y la instrucción.
// Uso (headless): -postScript ScalarRefs.java <out.txt> <valor> [<valor>...]   (hex con 0x o decimal)
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.scalar.Scalar;

public class ScalarRefs extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		long[] wanted = new long[args.length - 1];
		for (int i = 1; i < args.length; i++) {
			String a = args[i].toLowerCase();
			wanted[i - 1] = a.startsWith("0x") ? Long.parseLong(a.substring(2), 16) : Long.parseLong(a);
		}
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			InstructionIterator it = currentProgram.getListing().getInstructions(true);
			while (it.hasNext() && !monitor.isCancelled()) {
				Instruction ins = it.next();
				for (int op = 0; op < ins.getNumOperands(); op++) {
					for (Object o : ins.getOpObjects(op)) {
						if (!(o instanceof Scalar)) {
							continue;
						}
						long v = ((Scalar) o).getUnsignedValue();
						for (long w : wanted) {
							if (v == w) {
								Function f = getFunctionContaining(ins.getAddress());
								out.printf("0x%X\t%s\t%s\t%s%n", w, ins.getAddress(), f == null ? "?" : f.getName(), ins);
							}
						}
					}
				}
			}
		}
		println("out: " + args[0]);
	}
}
