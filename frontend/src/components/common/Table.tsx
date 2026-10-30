import type { HTMLAttributes, TableHTMLAttributes } from "react";
import { cn } from "../../lib/cn";

export function Table({
  className,
  children,
  ...rest
}: TableHTMLAttributes<HTMLTableElement>) {
  return (
    <div className="w-full overflow-x-auto rounded-xl border border-line/70 bg-raised">
      <table className={cn("w-full border-collapse text-sm", className)} {...rest}>
        {children}
      </table>
    </div>
  );
}

export function THead({ className, children, ...rest }: HTMLAttributes<HTMLTableSectionElement>) {
  return (
    <thead
      className={cn("border-b border-line/70 bg-paper/60 text-left text-xs text-muted", className)}
      {...rest}
    >
      {children}
    </thead>
  );
}

export function TBody({ className, children, ...rest }: HTMLAttributes<HTMLTableSectionElement>) {
  return (
    <tbody className={cn("divide-y divide-line/60", className)} {...rest}>
      {children}
    </tbody>
  );
}

export function TR({ className, children, ...rest }: HTMLAttributes<HTMLTableRowElement>) {
  return (
    <tr className={cn("transition-colors duration-150 hover:bg-accent-soft/20", className)} {...rest}>
      {children}
    </tr>
  );
}

export function TH({ className, children, ...rest }: HTMLAttributes<HTMLTableCellElement>) {
  return (
    <th className={cn("whitespace-nowrap px-3 py-2.5 font-medium", className)} {...rest}>
      {children}
    </th>
  );
}

export function TD({ className, children, ...rest }: HTMLAttributes<HTMLTableCellElement>) {
  return (
    <td className={cn("px-3 py-2.5 align-middle", className)} {...rest}>
      {children}
    </td>
  );
}
