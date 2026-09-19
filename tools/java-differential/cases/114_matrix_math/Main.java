public class Main {
    static long[][] multiply(long[][] a, long[][] b) {
        int n = a.length;
        int m = b[0].length;
        int k = b.length;
        long[][] out = new long[n][m];
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < m; j++) {
                long sum = 0;
                for (int t = 0; t < k; t++) {
                    sum += a[i][t] * b[t][j];
                }
                out[i][j] = sum;
            }
        }
        return out;
    }

    static long[][] identity(int n) {
        long[][] out = new long[n][n];
        for (int i = 0; i < n; i++) {
            out[i][i] = 1;
        }
        return out;
    }

    static long[][] transpose(long[][] a) {
        long[][] out = new long[a[0].length][a.length];
        for (int i = 0; i < a.length; i++) {
            for (int j = 0; j < a[0].length; j++) {
                out[j][i] = a[i][j];
            }
        }
        return out;
    }

    static long[][] power(long[][] base, int exp) {
        long[][] result = identity(base.length);
        long[][] b = base;
        int e = exp;
        while (e > 0) {
            if (e % 2 == 1) {
                result = multiply(result, b);
            }
            b = multiply(b, b);
            e = e / 2;
        }
        return result;
    }

    static long determinant(long[][] a) {
        int n = a.length;
        if (n == 1) {
            return a[0][0];
        }
        if (n == 2) {
            return a[0][0] * a[1][1] - a[0][1] * a[1][0];
        }
        long det = 0;
        for (int col = 0; col < n; col++) {
            long[][] minor = new long[n - 1][n - 1];
            for (int i = 1; i < n; i++) {
                int mc = 0;
                for (int j = 0; j < n; j++) {
                    if (j == col) {
                        continue;
                    }
                    minor[i - 1][mc] = a[i][j];
                    mc++;
                }
            }
            long sign = col % 2 == 0 ? 1 : -1;
            det += sign * a[0][col] * determinant(minor);
        }
        return det;
    }

    static double[] solve(double[][] m, double[] rhs) {
        int n = rhs.length;
        double[][] a = new double[n][n + 1];
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                a[i][j] = m[i][j];
            }
            a[i][n] = rhs[i];
        }
        for (int col = 0; col < n; col++) {
            int pivot = col;
            for (int r = col + 1; r < n; r++) {
                if (Math.abs(a[r][col]) > Math.abs(a[pivot][col])) {
                    pivot = r;
                }
            }
            double[] tmp = a[col];
            a[col] = a[pivot];
            a[pivot] = tmp;
            for (int r = 0; r < n; r++) {
                if (r == col) {
                    continue;
                }
                double factor = a[r][col] / a[col][col];
                for (int c = col; c <= n; c++) {
                    a[r][c] -= factor * a[col][c];
                }
            }
        }
        double[] x = new double[n];
        for (int i = 0; i < n; i++) {
            x[i] = a[i][n] / a[i][i];
        }
        return x;
    }

    static void show(String label, long[][] a) {
        System.out.println(label);
        for (long[] row : a) {
            String line = "";
            for (int j = 0; j < row.length; j++) {
                if (j > 0) {
                    line += " ";
                }
                line += row[j];
            }
            System.out.println("  " + line);
        }
    }

    public static void main(String[] args) {
        long[][] a = {{1, 2}, {3, 4}};
        long[][] b = {{0, 1}, {1, 0}};
        show("a*b", multiply(a, b));
        show("b*a", multiply(b, a));
        show("a^T", transpose(a));
        long[][] fib = {{1, 1}, {1, 0}};
        show("fib^10", power(fib, 10));
        long[][] rect = {{1, 2, 3}, {4, 5, 6}};
        show("rect*rect^T", multiply(rect, transpose(rect)));
        long[][] m3 = {{2, -3, 1}, {2, 0, -1}, {1, 4, 5}};
        System.out.println("det m3 = " + determinant(m3));
        long[][] m4 = {{1, 0, 2, -1}, {3, 0, 0, 5}, {2, 1, 4, -3}, {1, 0, 5, 0}};
        System.out.println("det m4 = " + determinant(m4));
        double[][] sys = {{2.0, 1.0, -1.0}, {-3.0, -1.0, 2.0}, {-2.0, 1.0, 2.0}};
        double[] rhs = {8.0, -11.0, -3.0};
        double[] x = solve(sys, rhs);
        for (int i = 0; i < x.length; i++) {
            System.out.println("x" + i + " = " + Math.round(x[i] * 1000.0) / 1000.0);
        }
    }
}
