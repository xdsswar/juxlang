public class Main {
    public static void main(String[] args) {
        String csv = "alpha,beta,gamma,delta";
        String[] parts = csv.split(",");
        System.out.println(parts.length);
        System.out.println(parts[0]);
        System.out.println(parts[3]);

        String joined = "";
        for (int i = 0; i < parts.length; i++) {
            if (i > 0) { joined = joined + "|"; }
            joined = joined + parts[i].toUpperCase();
        }
        System.out.println(joined);

        String digits = "  90210  ";
        String t = digits.trim();
        System.out.println(t.length());
        int value = 0;
        for (char c : t.toCharArray()) {
            value = value * 10 + ((int) c - (int) '0');
        }
        System.out.println(value);

        System.out.println(csv.indexOf("beta"));
        System.out.println(csv.replace(",", " / "));
        System.out.println(csv.contains("gamma"));
        System.out.println(csv.startsWith("alpha"));
        System.out.println(csv.substring(6, 10));

        System.out.println("n=" + 42 + " d=" + 1.5 + " b=" + true);
        System.out.println("" + 1 + 2 + 3);
        System.out.println(1 + 2 + "3");
    }
}
