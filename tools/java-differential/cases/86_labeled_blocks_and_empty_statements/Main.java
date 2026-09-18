public class Main {
    static class Scan {
        static int steps = 0;

        static boolean step() {
            steps = steps + 1;
            return steps < 5;
        }
    }

    static int indexOf(int[] values, int wanted) {
        int found = -1;
        search: {
            for (int i = 0; i < values.length; i++) {
                if (values[i] == wanted) {
                    found = i;
                    break search;
                }
            }
            System.out.println("not found: " + wanted);
        }
        return found;
    }

    public static void main(String[] args) {
        int[] values = {3, 9, 4, 7};
        System.out.println(indexOf(values, 4));
        System.out.println(indexOf(values, 5));

        for (int i = 0; i < 3; i++) {
            check: {
                if (i == 1) {
                    break check;
                }
                System.out.println("checked " + i);
            }
        }

        outer: for (int row = 0; row < 3; row++) {
            for (int col = 0; col < 3; col++) {
                if (row * col == 2) {
                    System.out.println("stop at " + row + "," + col);
                    break outer;
                }
            }
        }

        while (Scan.step());
        System.out.println(Scan.steps);;
        ;
        System.out.println("end");
    }
}
