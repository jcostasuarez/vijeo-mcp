// Instrucciones que escriben en [registro + desplazamiento] (p.ej. un miembro de una clase) y la función
// que las contiene. Sirve para saber quién activa un indicador visto en el pseudocódigo (`this + 0x1c8`).
// Uso (headless): -postScript StoresToOffset.java <out.txt> <desplazamiento hex> [...]
// @category VijeoMCP
import java.io.FileWriter;
import java.io.PrintWriter;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.lang.OperandType;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.scalar.Scalar;

public class StoresToOffset extends GhidraScript {
	@Override
	public void run() throws Exception {
		String[] args = getScriptArgs();
		if (args.length < 2) {
			printerr("uso: StoresToOffset <out.txt> <desplazamiento hex> [...]");
			return;
		}
		long[] offs = new long[args.length - 1];
		for (int i = 1; i < args.length; i++) {
			offs[i - 1] = Long.parseLong(args[i].replaceFirst("^0x", ""), 16);
		}
		try (PrintWriter out = new PrintWriter(new FileWriter(args[0]))) {
			InstructionIterator it = currentProgram.getListing().getInstructions(true);
			while (it.hasNext() && !monitor.isCancelled()) {
				Instruction ins = it.next();
				if (!ins.getMnemonicString().startsWith("MOV") || ins.getNumOperands() < 2) {
					continue;
				}
				int t = ins.getOperandType(0);
				if ((t & OperandType.DYNAMIC) == 0) {
					continue;
				}
				for (Object o : ins.getOpObjects(0)) {
					if (o instanceof Scalar s) {
						for (long off : offs) {
							if (s.getUnsignedValue() == off) {
								Function f = getFunctionContaining(ins.getAddress());
								out.println(ins.getAddress() + "\t" + ins + "\t" + (f != null ? f.getName() + "@" + f.getEntryPoint() : "?"));
							}
						}
					}
				}
			}
		}
	}
}
