import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public class Main {
    record Line(String ref, long cents, String memo) {}

    static class ReconcileError extends Exception {
        ReconcileError(String m) { super(m); }
    }

    static String money(long cents) {
        String sign = cents < 0 ? "-" : "";
        long abs = cents < 0 ? -cents : cents;
        long whole = abs / 100;
        long part = abs % 100;
        return sign + whole + "." + (part < 10 ? "0" : "") + part;
    }

    static String pad(String s, int width) {
        String out = s;
        while (out.length() < width) {
            out = out + " ";
        }
        return out;
    }

    static TreeMap<String, Line> index(List<Line> lines) throws ReconcileError {
        TreeMap<String, Line> map = new TreeMap<>();
        for (Line l : lines) {
            if (map.containsKey(l.ref())) {
                throw new ReconcileError("duplicate reference " + l.ref());
            }
            map.put(l.ref(), l);
        }
        return map;
    }

    static void reconcile(List<Line> ledger, List<Line> bank) {
        try {
            TreeMap<String, Line> ours = index(ledger);
            TreeMap<String, Line> theirs = index(bank);
            long matched = 0;
            long difference = 0;
            int problems = 0;
            for (Map.Entry<String, Line> entry : ours.entrySet()) {
                Line mine = entry.getValue();
                Line other = theirs.get(entry.getKey());
                if (other == null) {
                    System.out.println(pad(mine.ref(), 6) + " missing from bank   " + money(mine.cents()));
                    problems++;
                    difference += mine.cents();
                } else if (other.cents() != mine.cents()) {
                    long gap = other.cents() - mine.cents();
                    System.out.println(pad(mine.ref(), 6) + " amount differs      " + money(gap));
                    problems++;
                    difference += gap;
                } else {
                    matched += mine.cents();
                }
            }
            for (Map.Entry<String, Line> entry : theirs.entrySet()) {
                if (!ours.containsKey(entry.getKey())) {
                    System.out.println(pad(entry.getKey(), 6) + " missing from ledger " + money(entry.getValue().cents()));
                    problems++;
                    difference -= entry.getValue().cents();
                }
            }
            System.out.println("matched " + money(matched) + ", " + problems + " problem(s), net " + money(difference));
        } catch (ReconcileError e) {
            System.out.println("cannot reconcile: " + e.getMessage());
        }
    }

    public static void main(String[] args) {
        List<Line> ledger = new ArrayList<>();
        ledger.add(new Line("R104", 12500, "rent share"));
        ledger.add(new Line("R101", 4999, "phone"));
        ledger.add(new Line("R103", -2000, "refund"));
        ledger.add(new Line("R102", 780, "coffee"));

        List<Line> bank = new ArrayList<>();
        bank.add(new Line("R101", 4999, "PHONE CO"));
        bank.add(new Line("R102", 870, "CAFE"));
        bank.add(new Line("R105", 1500, "FEE"));
        bank.add(new Line("R104", 12500, "TRANSFER"));

        reconcile(ledger, bank);

        List<Line> twice = new ArrayList<>();
        twice.add(new Line("R1", 1, "a"));
        twice.add(new Line("R1", 2, "b"));
        reconcile(twice, bank);
    }
}
