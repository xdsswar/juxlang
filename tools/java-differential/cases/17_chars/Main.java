public class Main {
    static boolean isDigitCh(char c) {
        return c >= '0' && c <= '9';
    }

    public static void main(String[] args) {
        String s = "a1b22c333";
        int digits = 0;
        int letters = 0;
        for (char c : s.toCharArray()) {
            if (isDigitCh(c)) {
                digits = digits + 1;
            } else {
                letters = letters + 1;
            }
        }
        System.out.println(digits);
        System.out.println(letters);

        int value = 0;
        for (char c : "4207".toCharArray()) {
            value = value * 10 + ((int) c - (int) '0');
        }
        System.out.println(value);

        System.out.println('a' < 'b');
        System.out.println('Z' < 'a');
        System.out.println((char) ((int) 'a' + 1));
    }
}
